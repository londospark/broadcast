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
 *   NVAFX_MODEL_PATH Full path to a specific denoiser_48k*.trtpkg model file
 *   NVAFX_SM         GPU SM version override, e.g. "120" for RTX 50xx
 */

#define _GNU_SOURCE
#include <ladspa.h>
#include <nvAudioEffects.h>
#include <denoiser.h>
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

typedef struct {
    NvAFX_Handle    effect;
    NvAFX_Handle    effect_r;   /* right channel for stereo */
    int             is_stereo;
    int             initialized; /* lazy-init guard */
    char            model_path[4096];
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
 *   1. NVAFX_MODEL_PATH env var (full path to .trtpkg)
 *   2. $NVAFX_SDK/features/denoiser/models/sm_$NVAFX_SM/denoiser_48k.trtpkg
 *   3. Walk $NVAFX_SDK/features/denoiser/models/ for any sm_* dir with denoiser_48k.trtpkg
 */
static int try_model_path(char *buf, size_t n, const char *dir, const char *file) {
    snprintf(buf, n, "%s/%s", dir, file);
    FILE *f = fopen(buf, "rb");
    if (f) { fclose(f); return 1; }
    return 0;
}

static const char *find_model_path(char *buf, size_t n) {
    /* 1. Explicit override */
    const char *explicit = getenv("NVAFX_MODEL_PATH");
    if (explicit && *explicit) {
        snprintf(buf, n, "%s", explicit);
        return buf;
    }

    const char *sdk = getenv("NVAFX_SDK");
    if (!sdk || !*sdk) return NULL;

    char models_dir[4096];
    snprintf(models_dir, sizeof(models_dir), "%s/features/denoiser/models", sdk);

    /* 2. Use NVAFX_SM env override */
    const char *sm_override = getenv("NVAFX_SM");
    if (sm_override && *sm_override) {
        char sm_dir[4096];
        snprintf(sm_dir, sizeof(sm_dir), "%s/sm_%s", models_dir, sm_override);
        if (try_model_path(buf, n, sm_dir, "denoiser_48k.trtpkg")) return buf;
        /* Try the v2 model as fallback */
        if (try_model_path(buf, n, sm_dir, "denoiser_v2_48k.trtpkg")) return buf;
    }

    /* 3. Walk the models directory for any sm_* subdirectory */
    DIR *d = opendir(models_dir);
    if (!d) {
        fprintf(stderr, "broadcast-maxine: models dir not found: %s\n", models_dir);
        return NULL;
    }
    struct dirent *entry;
    while ((entry = readdir(d)) != NULL) {
        if (strncmp(entry->d_name, "sm_", 3) != 0) continue;
        char sm_dir[4096];
        snprintf(sm_dir, sizeof(sm_dir), "%s/%s", models_dir, entry->d_name);
        if (try_model_path(buf, n, sm_dir, "denoiser_48k.trtpkg")) {
            closedir(d);
            return buf;
        }
        if (try_model_path(buf, n, sm_dir, "denoiser_v2_48k.trtpkg")) {
            closedir(d);
            return buf;
        }
    }
    closedir(d);
    fprintf(stderr, "broadcast-maxine: no denoiser_48k.trtpkg found under %s\n", models_dir);
    return NULL;
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
static NvAFX_Handle create_denoiser(const char *model_path) {
    NvAFX_Handle h = NULL;
    NvAFX_Status st;

    ensure_sdk_global();

    st = NvAFX_CreateEffect(NVAFX_EFFECT_DENOISER, &h);
    if (st != NVAFX_STATUS_SUCCESS) {
        fprintf(stderr, "broadcast-maxine: NvAFX_CreateEffect failed: %d\n", st);
        return NULL;
    }

    /* SDK v2.x requires sample rate and stream count before NvAFX_Load */
    NvAFX_SetU32(h, NVAFX_PARAM_INPUT_SAMPLE_RATE, 48000);
    NvAFX_SetU32(h, NVAFX_PARAM_NUM_STREAMS, 1);

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

    char model_buf[4096];
    const char *model_path = find_model_path(model_buf, sizeof(model_buf));
    if (!model_path) {
        fprintf(stderr, "broadcast-maxine: could not locate denoiser model.\n"
                        "  Set NVAFX_MODEL_PATH=/path/to/denoiser_48k.trtpkg\n"
                        "  or NVAFX_SDK=/path/to/Audio_Effects_SDK\n");
        free(inst);
        return NULL;
    }

    /* Store path for deferred load in activate() */
    snprintf(inst->model_path, sizeof(inst->model_path), "%s", model_path);
    fprintf(stderr, "broadcast-maxine: instantiate ok, model will load on activate: %s\n",
            inst->model_path);

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

    fprintf(stderr, "broadcast-maxine: activate — loading model (serialised): %s\n",
            inst->model_path);

    pthread_mutex_lock(&g_trt_load_mutex);
    inst->effect = create_denoiser(inst->model_path);
    if (inst->is_stereo && inst->effect) {
        inst->effect_r = create_denoiser(inst->model_path);
        if (!inst->effect_r) {
            fprintf(stderr, "broadcast-maxine: stereo R channel load failed, "
                            "falling back to mono-duplicated output\n");
            /* Fall back gracefully — run() will duplicate L→R */
        }
    }
    pthread_mutex_unlock(&g_trt_load_mutex);

    if (!inst->effect) {
        fprintf(stderr, "broadcast-maxine: effect load failed — running in passthrough mode\n");
    } else {
        fprintf(stderr, "broadcast-maxine: effect loaded ok%s\n",
                inst->is_stereo ? " (stereo)" : " (mono)");
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
