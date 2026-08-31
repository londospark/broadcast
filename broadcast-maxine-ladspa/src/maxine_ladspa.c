/**
 * NVIDIA Maxine Audio Effects LADSPA wrapper (SDK v2.x)
 *
 * Exposes two LADSPA plugins:
 *   - maxine_denoiser_mono   (index 0) — for mic input (PipeWire capture filter chain)
 *   - maxine_denoiser_stereo (index 1) — for speaker output (PipeWire output filter chain)
 *
 * Build requirements:
 *   - NVIDIA Audio Effects SDK v2.x: NGC nvidia/maxine/maxine_linux_audio_effects_sdk
 *   - Denoiser feature package for your GPU (e.g. rtx_pro_6000 = SM120 for RTX 50xx)
 *
 * Runtime requirements:
 *   - NVIDIA GPU with Tensor Cores (RTX 20xx+), driver >= 570
 *   - NVAFX_SDK env var pointing to Audio_Effects_SDK directory
 *   - Model .trtpkg file discovered via NVAFX_MODEL_PATH or auto-detected from NVAFX_SDK
 *
 * Environment variables (all optional — auto-detected from NVAFX_SDK when possible):
 *   NVAFX_SDK        Path to Audio_Effects_SDK directory
 *   NVAFX_MODEL_PATH Full path to a specific model .trtpkg file
 *   NVAFX_SM         GPU SM version override, e.g. "120" for RTX 50xx
 *   NVAFX_EFFECT      "denoiser" (noise only) or "dereverb_denoiser" (noise +
 *                     room echo removal — closer to NVIDIA Broadcast's
 *                     Windows defaults). Auto-selects dereverb_denoiser when
 *                     that feature package was downloaded and its model is
 *                     found; set to "denoiser" to force plain denoising.
 */

#define _GNU_SOURCE
#include <ladspa.h>
#include <nvAudioEffects.h>
#include <denoiser.h>
#ifdef NVAFX_HAS_DEREVERB_DENOISER
#include <dereverb_denoiser.h>
#endif
#include <dirent.h>
#include <dlfcn.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Self-protect: prevent our own code from being unmapped after dlclose.
 * The SDK (NvAFX_Load) creates background CUDA/TRT threads. If PipeWire
 * calls dlclose on this plugin, the code stays mapped due to RTLD_NODELETE,
 * so those threads don't crash. Called automatically at library load time. */
static void __attribute__((constructor)) self_nodelete(void) {
    Dl_info info;
    if (dladdr((void*)self_nodelete, &info) && info.dli_fname)
        dlopen(info.dli_fname, RTLD_LAZY | RTLD_NODELETE | RTLD_GLOBAL);
}

/* Belt-and-suspenders: ensure libnvinfer_plugin is in the global symbol table
 * in case LD_PRELOAD wasn't used (e.g. running outside PipeWire service).
 * The real fix is LD_PRELOAD in the PipeWire systemd drop-in. */
static void ensure_sdk_global(void) {
    static int done = 0;
    if (done) return;
    done = 1;

    const char *sdk = getenv("NVAFX_SDK");
    if (!sdk || !*sdk) return;

    char path[4096];
    snprintf(path, sizeof(path), "%s/external/cuda/lib/libnvinfer_plugin.so.10", sdk);
    dlopen(path, RTLD_GLOBAL | RTLD_LAZY);

#ifdef NVAFX_HAS_DEREVERB_DENOISER
    /* Re-open with RTLD_GLOBAL even though it's already link-time linked:
     * a feature .so's self-registration with the core nv_audiofx registry
     * can depend on static-init ordering that DT_NEEDED linking doesn't
     * guarantee across sibling libraries. An explicit RTLD_GLOBAL dlopen
     * forces it fully initialised before NvAFX_CreateEffect runs. */
    snprintf(path, sizeof(path), "%s/features/dereverb_denoiser/lib/libnv_audiofx_dereverb_denoiser.so", sdk);
    dlopen(path, RTLD_GLOBAL | RTLD_NOW);
#endif
}

/* ── LADSPA port indices ─────────────────────────────────────────────── */
#define PORT_AUDIO_IN_L  0
#define PORT_AUDIO_IN_R  1   /* only used in stereo variant */
#define PORT_AUDIO_OUT_L 2
#define PORT_AUDIO_OUT_R 3   /* only used in stereo variant */
#define PORT_INTENSITY   4   /* control: 0.0–1.0, default 1.0 */

#define NUM_SAMPLES_MONO 480  /* 10ms @ 48kHz */

/* ── Global mutex to serialise TRT engine loads ──────────────────────
 * PipeWire loads both filter-chain configs at startup. Two concurrent
 * NvAFX_Load calls on the same model race inside TensorRT and the
 * second one fails. The mutex ensures they happen one at a time. */
static pthread_mutex_t g_trt_load_mutex = PTHREAD_MUTEX_INITIALIZER;

/* ── Instance data ───────────────────────────────────────────────────── */

/* Ring buffer capacity: must hold at least one PipeWire quantum (typically 1024)
 * plus one extra Maxine frame (480), with headroom. 8 × 480 = 3840 samples. */
#define RING_SIZE (480 * 8)

/* One (model path, effect) pair to try loading. effect_version is 0 (leave
 * at the SDK default) or a version number to request explicitly via
 * NVAFX_PARAM_EFFECT_VERSION — required for the "v2" denoiser model to
 * actually load correctly, per NVIDIA's own effects_demo sample; without
 * it NvAFX_Load fails even though the v2 .trtpkg file is valid. */
typedef struct {
    char path[4096];
    int  use_dereverb;
    int  effect_version;
} ModelCandidate;

#define MAX_CANDIDATES 3

typedef struct {
    NvAFX_Handle    effect;
    NvAFX_Handle    effect_r;   /* right channel for stereo */
    int             is_stereo;
    int             initialized; /* lazy-init guard */
    /* Ordered, most-preferred-first list of (model, effect) candidates.
     * activate() tries each in turn — a candidate's model file existing on
     * disk doesn't guarantee NvAFX_CreateEffect/Load will actually succeed
     * for it at runtime, so falling through to the next is what keeps a
     * stream processed instead of silently passing through unfiltered. */
    ModelCandidate  candidates[MAX_CANDIDATES];
    int             n_candidates;
    int             active_candidate; /* index into candidates that loaded, once known */
    LADSPA_Data    *port_in_l;
    LADSPA_Data    *port_in_r;
    LADSPA_Data    *port_out_l;
    LADSPA_Data    *port_out_r;
    LADSPA_Data    *port_intensity;
    /* Scratch buffers (exactly NUM_SAMPLES_MONO) for NvAFX_Run */
    float          *in_buf_l;
    float          *in_buf_r;
    float          *out_buf_l;
    float          *out_buf_r;
    /* Ring buffers: decouple PipeWire quantum from Maxine's 480-sample frame.
     * Without these, any quantum that isn't a multiple of 480 leaves a leftover
     * tail that is either copied raw (causes robotic artefacts due to model state
     * discontinuity) or dropped.  The rings keep the model fed continuously. */
    float          *in_ring_l;
    float          *in_ring_r;
    float          *out_ring_l;
    float          *out_ring_r;
    int             in_wpos;    /* write position in input ring  */
    int             in_rpos;    /* read  position in input ring  */
    int             in_avail;   /* samples available in input ring */
    int             out_wpos;   /* write position in output ring */
    int             out_rpos;   /* read  position in output ring */
    int             out_avail;  /* samples available in output ring */
} MaxineInstance;

/* ── Model path discovery ─────────────────────────────────────────────
 * Priority:
 *   1. NVAFX_MODEL_PATH env var (full path to .trtpkg) — effect is assumed
 *      to be plain denoiser in this case, since we can't infer it from a
 *      path.
 *   2. dereverb_denoiser, if that feature was linked in at build time and
 *      NVAFX_EFFECT hasn't forced plain denoiser (see header comment).
 *   3. Plain denoiser — preferring the "v2" model over the original when
 *      both are present. v2 is a newer retrain with noticeably better
 *      suppression of sharp transients (keyboard/mouse clicks) than v1.
 * Within each effect, searches $NVAFX_SDK/features/<effect>/models/sm_$NVAFX_SM
 * first, then walks models/ for any sm_* directory.
 */
static int try_model_path(char *buf, size_t n, const char *dir, const char *file) {
    snprintf(buf, n, "%s/%s", dir, file);
    FILE *f = fopen(buf, "rb");
    if (f) { fclose(f); return 1; }
    return 0;
}

/* Search one effect's models/ tree for the first filename (in preference
 * order) that exists. Returns 1 and fills buf on success. */
static int find_effect_model(char *buf, size_t n, const char *sdk,
                              const char *feature_dir,
                              const char **filenames, int n_filenames) {
    char models_dir[4096];
    snprintf(models_dir, sizeof(models_dir), "%s/features/%s/models", sdk, feature_dir);

    const char *sm_override = getenv("NVAFX_SM");
    if (sm_override && *sm_override) {
        char sm_dir[4096];
        snprintf(sm_dir, sizeof(sm_dir), "%s/sm_%s", models_dir, sm_override);
        for (int i = 0; i < n_filenames; i++)
            if (try_model_path(buf, n, sm_dir, filenames[i])) return 1;
    }

    DIR *d = opendir(models_dir);
    if (!d) return 0;
    struct dirent *entry;
    int found = 0;
    while (!found && (entry = readdir(d)) != NULL) {
        if (strncmp(entry->d_name, "sm_", 3) != 0) continue;
        char sm_dir[4096];
        snprintf(sm_dir, sizeof(sm_dir), "%s/%s", models_dir, entry->d_name);
        for (int i = 0; i < n_filenames; i++) {
            if (try_model_path(buf, n, sm_dir, filenames[i])) {
                found = 1;
                break;
            }
        }
    }
    closedir(d);
    return found;
}

/* Builds an ordered, most-preferred-first list of (model, effect)
 * candidates for activate() to try loading in turn:
 *   1. NVAFX_MODEL_PATH env var, if set — a single explicit candidate,
 *      assumed to be a plain denoiser model.
 *   2. dereverb_denoiser (noise + room echo removal — closer to what
 *      NVIDIA Broadcast applies by default on Windows), if that feature
 *      was linked in at build time, its model is present, and NVAFX_EFFECT
 *      hasn't forced plain denoiser.
 *   3. Plain denoiser, "v2" model — a newer retrain with noticeably better
 *      suppression of sharp transients (keyboard/mouse clicks) than v1.
 *   4. Plain denoiser, original "v1" model.
 * Every entry a build can find gets a slot; activate() is what actually
 * discovers which ones the SDK will initialise at runtime and falls
 * through accordingly, since a model's presence on disk doesn't guarantee
 * NvAFX_CreateEffect/Load will succeed for it.
 */
static int build_candidates(ModelCandidate *out, int max_out) {
    int count = 0;

    const char *explicit = getenv("NVAFX_MODEL_PATH");
    if (explicit && *explicit) {
        snprintf(out[0].path, sizeof(out[0].path), "%s", explicit);
        out[0].use_dereverb = 0;
        out[0].effect_version = 0;
        return 1;
    }

    const char *sdk = getenv("NVAFX_SDK");
    if (!sdk || !*sdk) return 0;

#ifdef NVAFX_HAS_DEREVERB_DENOISER
    const char *effect_env = getenv("NVAFX_EFFECT");
    int want_dereverb = !(effect_env && strcmp(effect_env, "denoiser") == 0);
    if (want_dereverb && count < max_out) {
        static const char *f[] = { "dereverb_denoiser_48k.trtpkg" };
        if (find_effect_model(out[count].path, sizeof(out[count].path), sdk,
                               "dereverb_denoiser", f, 1)) {
            out[count].use_dereverb = 1;
            out[count].effect_version = 0;
            count++;
        }
    }
#endif

    if (count < max_out) {
        static const char *f[] = { "denoiser_v2_48k.trtpkg" };
        if (find_effect_model(out[count].path, sizeof(out[count].path), sdk, "denoiser", f, 1)) {
            out[count].use_dereverb = 0;
            out[count].effect_version = 2; /* requires NVAFX_PARAM_EFFECT_VERSION=2 to load */
            count++;
        }
    }
    if (count < max_out) {
        static const char *f[] = { "denoiser_48k.trtpkg" };
        if (find_effect_model(out[count].path, sizeof(out[count].path), sdk, "denoiser", f, 1)) {
            out[count].use_dereverb = 0;
            out[count].effect_version = 0;
            count++;
        }
    }

    if (count == 0) {
        fprintf(stderr, "broadcast-maxine: no denoiser model found under %s/features/*/models\n", sdk);
    }
    return count;
}

/* ── Ring buffer helpers ──────────────────────────────────────────────
 * Low-level read/write that do NOT touch counters — caller advances them.
 * L and R rings are always kept in lockstep, so they share one set of
 * position/avail variables. */
static void ring_write(float *ring, int wpos, const float *src, int n) {
    for (int i = 0; i < n; i++)
        ring[(wpos + i) % RING_SIZE] = src[i];
}

static void ring_read(float *ring, int rpos, float *dst, int n) {
    for (int i = 0; i < n; i++)
        dst[i] = ring[(rpos + i) % RING_SIZE];
}

/* ── Helpers ──────────────────────────────────────────────────────────── */
static NvAFX_Handle create_denoiser(const char *model_path, int use_dereverb, int effect_version) {
    NvAFX_Handle h = NULL;
    NvAFX_Status st;
    const char *effect_selector = NVAFX_EFFECT_DENOISER;

#ifdef NVAFX_HAS_DEREVERB_DENOISER
    if (use_dereverb) effect_selector = NVAFX_EFFECT_DEREVERB_DENOISER;
#else
    (void)use_dereverb;
#endif

    ensure_sdk_global();

    st = NvAFX_CreateEffect(effect_selector, &h);
    if (st != NVAFX_STATUS_SUCCESS) {
        fprintf(stderr, "broadcast-maxine: NvAFX_CreateEffect failed: %d\n", st);
        return NULL;
    }

    /* SDK v2.x requires sample rate and stream count before NvAFX_Load */
    NvAFX_SetU32(h, NVAFX_PARAM_INPUT_SAMPLE_RATE, 48000);
    NvAFX_SetU32(h, NVAFX_PARAM_NUM_STREAMS, 1);

    /* Required for the "v2" denoiser model specifically — without this the
     * SDK validates against effect_version 1 (its default) and NvAFX_Load
     * fails on a v2 .trtpkg even though the file itself is valid. Matches
     * NVIDIA's own effects_demo sample. */
    if (effect_version) {
        st = NvAFX_SetU32(h, NVAFX_PARAM_EFFECT_VERSION, (unsigned)effect_version);
        if (st != NVAFX_STATUS_SUCCESS) {
            fprintf(stderr, "broadcast-maxine: NvAFX_SetU32(effect_version=%d) failed: %d\n",
                    effect_version, st);
            NvAFX_DestroyEffect(h);
            return NULL;
        }
    }

    st = NvAFX_SetString(h, NVAFX_PARAM_MODEL_PATH, model_path);
    if (st != NVAFX_STATUS_SUCCESS) {
        fprintf(stderr, "broadcast-maxine: NvAFX_SetString(model_path='%s') failed: %d\n",
                model_path, st);
        NvAFX_DestroyEffect(h);
        return NULL;
    }
    st = NvAFX_Load(h);
    if (st != NVAFX_STATUS_SUCCESS) {
        fprintf(stderr, "broadcast-maxine: NvAFX_Load failed: %d\n", st);
        NvAFX_DestroyEffect(h);
        return NULL;
    }
    return h;
}

/* ── LADSPA callbacks ────────────────────────────────────────────────── */
static LADSPA_Handle instantiate(const LADSPA_Descriptor *desc,
                                 unsigned long sample_rate) {
    (void)sample_rate; /* PipeWire must be configured for 48kHz */

    MaxineInstance *inst = (MaxineInstance *)calloc(1, sizeof(MaxineInstance));
    if (!inst) return NULL;

    inst->is_stereo = (desc->UniqueID == 2);  /* 1 = mono, 2 = stereo */

    inst->n_candidates = build_candidates(inst->candidates, MAX_CANDIDATES);
    if (inst->n_candidates == 0) {
        fprintf(stderr, "broadcast-maxine: could not locate a denoiser model.\n"
                        "  Set NVAFX_MODEL_PATH=/path/to/model.trtpkg\n"
                        "  or NVAFX_SDK=/path/to/Audio_Effects_SDK\n");
        free(inst);
        return NULL;
    }
    fprintf(stderr, "broadcast-maxine: instantiate ok, %d candidate model(s), "
                    "will load on activate (preferred: %s: %s)\n",
            inst->n_candidates,
            inst->candidates[0].use_dereverb ? "dereverb_denoiser" : "denoiser",
            inst->candidates[0].path);

    inst->in_buf_l  = (float *)malloc(NUM_SAMPLES_MONO * sizeof(float));
    inst->out_buf_l = (float *)malloc(NUM_SAMPLES_MONO * sizeof(float));
    inst->in_ring_l  = (float *)calloc(RING_SIZE, sizeof(float));
    inst->out_ring_l = (float *)calloc(RING_SIZE, sizeof(float));
    if (inst->is_stereo) {
        inst->in_buf_r  = (float *)malloc(NUM_SAMPLES_MONO * sizeof(float));
        inst->out_buf_r = (float *)malloc(NUM_SAMPLES_MONO * sizeof(float));
        inst->in_ring_r  = (float *)calloc(RING_SIZE, sizeof(float));
        inst->out_ring_r = (float *)calloc(RING_SIZE, sizeof(float));
    }

    return (LADSPA_Handle)inst;
}

/* Deferred TRT engine load — called by PipeWire after connect_port, before run.
 * Serialised with a mutex so that two filter chains starting simultaneously
 * don't both try to compile/cache the TRT engine at the same time. */
static void activate(LADSPA_Handle handle) {
    MaxineInstance *inst = (MaxineInstance *)handle;
    if (inst->initialized) return;
    inst->initialized = 1;

    pthread_mutex_lock(&g_trt_load_mutex);

    /* Try each candidate in preference order until one actually loads. A
     * candidate's model file existing on disk doesn't guarantee
     * NvAFX_CreateEffect/Load will succeed for it at runtime, so this is
     * what keeps the stream processed by falling through to the next
     * (ultimately the proven-working plain denoiser) instead of the
     * stream running unfiltered. */
    inst->active_candidate = -1;
    for (int i = 0; i < inst->n_candidates; i++) {
        ModelCandidate *c = &inst->candidates[i];
        fprintf(stderr, "broadcast-maxine: activate — trying candidate %d/%d (%s): %s\n",
                i + 1, inst->n_candidates, c->use_dereverb ? "dereverb_denoiser" : "denoiser",
                c->path);
        inst->effect = create_denoiser(c->path, c->use_dereverb, c->effect_version);
        if (inst->effect) {
            inst->active_candidate = i;
            break;
        }
        fprintf(stderr, "broadcast-maxine: candidate %d failed, trying next...\n", i + 1);
    }

    if (inst->active_candidate >= 0 && inst->is_stereo) {
        ModelCandidate *c = &inst->candidates[inst->active_candidate];
        inst->effect_r = create_denoiser(c->path, c->use_dereverb, c->effect_version);
        if (!inst->effect_r) {
            fprintf(stderr, "broadcast-maxine: stereo R channel load failed, "
                            "falling back to mono-duplicated output\n");
            /* Fall back gracefully — run() will duplicate L→R */
        }
    }
    pthread_mutex_unlock(&g_trt_load_mutex);

    if (inst->active_candidate < 0) {
        fprintf(stderr, "broadcast-maxine: all %d candidate(s) failed — "
                        "running in passthrough mode\n", inst->n_candidates);
    } else {
        ModelCandidate *c = &inst->candidates[inst->active_candidate];
        fprintf(stderr, "broadcast-maxine: effect loaded ok (%s%s): %s\n",
                c->use_dereverb ? "dereverb_denoiser" : "denoiser",
                inst->is_stereo ? ", stereo" : ", mono", c->path);
    }
}

static void connect_port(LADSPA_Handle handle, unsigned long port, LADSPA_Data *buf) {
    MaxineInstance *inst = (MaxineInstance *)handle;
    switch (port) {
        case PORT_AUDIO_IN_L:  inst->port_in_l       = buf; break;
        case PORT_AUDIO_IN_R:  inst->port_in_r        = buf; break;
        case PORT_AUDIO_OUT_L: inst->port_out_l       = buf; break;
        case PORT_AUDIO_OUT_R: inst->port_out_r       = buf; break;
        case PORT_INTENSITY:   inst->port_intensity   = buf; break;
    }
}

static void run(LADSPA_Handle handle, unsigned long sample_count) {
    MaxineInstance *inst = (MaxineInstance *)handle;
    if (!inst->port_in_l || !inst->port_out_l) return;

    /* Passthrough when denoiser failed to load */
    if (!inst->effect) {
        memcpy(inst->port_out_l, inst->port_in_l, sample_count * sizeof(float));
        if (inst->is_stereo && inst->port_in_r && inst->port_out_r)
            memcpy(inst->port_out_r, inst->port_in_r, sample_count * sizeof(float));
        return;
    }

    float intensity = inst->port_intensity ? *inst->port_intensity : 1.0f;
    if (intensity < 0.0f) intensity = 0.0f;
    if (intensity > 1.0f) intensity = 1.0f;

    int n = (int)sample_count;

    /* ── Step 1: push incoming samples into the input rings ─────────────
     * L and R are written at the SAME wpos; counters advance once.       */
    ring_write(inst->in_ring_l, inst->in_wpos, inst->port_in_l, n);
    if (inst->is_stereo && inst->in_ring_r && inst->port_in_r)
        ring_write(inst->in_ring_r, inst->in_wpos, inst->port_in_r, n);
    inst->in_wpos  = (inst->in_wpos + n) % RING_SIZE;
    inst->in_avail += n;

    /* ── Step 2: process all complete 480-sample frames ─────────────────
     * L and R are read at the SAME rpos; counters advance once per frame. */
    while (inst->in_avail >= NUM_SAMPLES_MONO) {
        ring_read(inst->in_ring_l, inst->in_rpos, inst->in_buf_l, NUM_SAMPLES_MONO);
        if (inst->is_stereo && inst->in_ring_r)
            ring_read(inst->in_ring_r, inst->in_rpos, inst->in_buf_r, NUM_SAMPLES_MONO);
        inst->in_rpos  = (inst->in_rpos + NUM_SAMPLES_MONO) % RING_SIZE;
        inst->in_avail -= NUM_SAMPLES_MONO;

        /* Process L channel */
        float *in_l[1]  = { inst->in_buf_l };
        float *out_l[1] = { inst->out_buf_l };
        NvAFX_Run(inst->effect, (const float **)in_l, out_l, NUM_SAMPLES_MONO, 1);
        for (int i = 0; i < NUM_SAMPLES_MONO; i++)
            inst->out_buf_l[i] = intensity * inst->out_buf_l[i]
                                + (1.0f - intensity) * inst->in_buf_l[i];

        /* Process R channel (or duplicate L) */
        float *ready_r = inst->out_buf_l; /* default: duplicate denoised L */
        if (inst->is_stereo && inst->effect_r && inst->in_buf_r) {
            float *in_r[1]  = { inst->in_buf_r };
            float *out_r[1] = { inst->out_buf_r };
            NvAFX_Run(inst->effect_r, (const float **)in_r, out_r, NUM_SAMPLES_MONO, 1);
            for (int i = 0; i < NUM_SAMPLES_MONO; i++)
                inst->out_buf_r[i] = intensity * inst->out_buf_r[i]
                                    + (1.0f - intensity) * inst->in_buf_r[i];
            ready_r = inst->out_buf_r;
        }

        /* Write processed frames to output rings at the SAME wpos */
        ring_write(inst->out_ring_l, inst->out_wpos, inst->out_buf_l, NUM_SAMPLES_MONO);
        if (inst->is_stereo && inst->out_ring_r)
            ring_write(inst->out_ring_r, inst->out_wpos, ready_r, NUM_SAMPLES_MONO);
        inst->out_wpos  = (inst->out_wpos + NUM_SAMPLES_MONO) % RING_SIZE;
        inst->out_avail += NUM_SAMPLES_MONO;
    }

    /* ── Step 3: pull output for this PipeWire cycle ─────────────────────
     * If the output ring is short (startup latency on the very first cycle)
     * pad with silence.  L and R are read at the SAME rpos.               */
    int from_ring = inst->out_avail < n ? inst->out_avail : n;
    int silence   = n - from_ring;

    ring_read(inst->out_ring_l, inst->out_rpos, inst->port_out_l, from_ring);
    if (inst->is_stereo && inst->port_out_r && inst->out_ring_r)
        ring_read(inst->out_ring_r, inst->out_rpos, inst->port_out_r, from_ring);
    inst->out_rpos  = (inst->out_rpos + from_ring) % RING_SIZE;
    inst->out_avail -= from_ring;

    if (silence > 0) {
        memset(inst->port_out_l + from_ring, 0, silence * sizeof(float));
        if (inst->is_stereo && inst->port_out_r)
            memset(inst->port_out_r + from_ring, 0, silence * sizeof(float));
    }
}

static void cleanup(LADSPA_Handle handle) {
    MaxineInstance *inst = (MaxineInstance *)handle;
    if (!inst) return;
    NvAFX_DestroyEffect(inst->effect);
    if (inst->effect_r) NvAFX_DestroyEffect(inst->effect_r);
    free(inst->in_buf_l);
    free(inst->out_buf_l);
    free(inst->in_buf_r);
    free(inst->out_buf_r);
    free(inst->in_ring_l);
    free(inst->in_ring_r);
    free(inst->out_ring_l);
    free(inst->out_ring_r);
    free(inst);
}

/* ── Port descriptors ────────────────────────────────────────────────── */
static const LADSPA_PortDescriptor MONO_PORT_DESCS[] = {
    LADSPA_PORT_INPUT  | LADSPA_PORT_AUDIO,  /* 0: audio in L */
    LADSPA_PORT_INPUT  | LADSPA_PORT_AUDIO,  /* 1: audio in R (unused mono) */
    LADSPA_PORT_OUTPUT | LADSPA_PORT_AUDIO,  /* 2: audio out L */
    LADSPA_PORT_OUTPUT | LADSPA_PORT_AUDIO,  /* 3: audio out R (unused mono) */
    LADSPA_PORT_INPUT  | LADSPA_PORT_CONTROL,/* 4: intensity */
};

static const char *MONO_PORT_NAMES[] = {
    "Input L", "Input R", "Output L", "Output R", "Intensity"
};

static const LADSPA_PortRangeHint MONO_PORT_HINTS[] = {
    { 0, 0.0f, 0.0f },
    { 0, 0.0f, 0.0f },
    { 0, 0.0f, 0.0f },
    { 0, 0.0f, 0.0f },
    { LADSPA_HINT_BOUNDED_BELOW | LADSPA_HINT_BOUNDED_ABOVE |
      LADSPA_HINT_DEFAULT_1,
      0.0f, 1.0f },
};

/* ── LADSPA descriptors ──────────────────────────────────────────────── */
static LADSPA_Descriptor g_mono_desc = {
    .UniqueID          = 1,
    .Label             = "maxine_denoiser_mono",
    .Properties        = LADSPA_PROPERTY_REALTIME | LADSPA_PROPERTY_INPLACE_BROKEN,
    .Name              = "NVIDIA Maxine Denoiser (Mono)",
    .Maker             = "broadcast / NVIDIA",
    .Copyright         = "GPL-2.0",
    .PortCount         = 5,
    .PortDescriptors   = MONO_PORT_DESCS,
    .PortNames         = MONO_PORT_NAMES,
    .PortRangeHints    = MONO_PORT_HINTS,
    .instantiate       = instantiate,
    .connect_port      = connect_port,
    .activate          = activate,
    .run               = run,
    .cleanup           = cleanup,
    /* optional: */
    .run_adding        = NULL,
    .set_run_adding_gain = NULL,
    .deactivate        = NULL,
    .ImplementationData = NULL,
};

static LADSPA_Descriptor g_stereo_desc = {
    .UniqueID          = 2,
    .Label             = "maxine_denoiser_stereo",
    .Properties        = LADSPA_PROPERTY_REALTIME | LADSPA_PROPERTY_INPLACE_BROKEN,
    .Name              = "NVIDIA Maxine Denoiser (Stereo)",
    .Maker             = "broadcast / NVIDIA",
    .Copyright         = "GPL-2.0",
    .PortCount         = 5,
    .PortDescriptors   = MONO_PORT_DESCS,
    .PortNames         = MONO_PORT_NAMES,
    .PortRangeHints    = MONO_PORT_HINTS,
    .instantiate       = instantiate,
    .connect_port      = connect_port,
    .activate          = activate,
    .run               = run,
    .cleanup           = cleanup,
    .run_adding        = NULL,
    .set_run_adding_gain = NULL,
    .deactivate        = NULL,
    .ImplementationData = NULL,
};

/* ── Entry point required by LADSPA ─────────────────────────────────── */
/* Named _maxine_ladspa_descriptor_impl so the Rust cdylib shim in lib.rs
 * can re-export it as the public `ladspa_descriptor` symbol. */
const LADSPA_Descriptor *_maxine_ladspa_descriptor_impl(unsigned long index) {
    switch (index) {
        case 0: return &g_mono_desc;
        case 1: return &g_stereo_desc;
        default: return NULL;
    }
}
