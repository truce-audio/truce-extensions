// lv2_smoke - load one LV2 bundle through lilv the way a host does, and
// run the plugin it holds.
//
//   lv2_smoke <bundle.lv2> [--passthrough]
//
// Checks, failing (exit 1) on the first that doesn't hold:
//   - the bundle holds exactly one plugin, and lilv_plugin_verify accepts it
//   - it instantiates with only urid:map, the one feature truce plugins need
//   - with every port connected (a distinct constant on each audio input,
//     control ports at their defaults, empty atom sequences in, room for
//     events out) it activates and runs two blocks
//   - every audio output sample is finite
//   - with --passthrough, each audio output equals the input of the same
//     index (for a plugin that copies its input, like the LV2 example)
//
// Built against lilv in CI (scripts/ci/lv2-check.sh); not part of any crate.

#include <lilv/lilv.h>
#include <lv2/atom/atom.h>
#include <lv2/urid/urid.h>

#include <limits.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define BLOCK 256
#define ATOM_CAPACITY 8192

static char **uris = NULL;
static size_t n_uris = 0;

static LV2_URID map_uri(LV2_URID_Map_Handle handle, const char *uri) {
    (void)handle;
    for (size_t i = 0; i < n_uris; i++) {
        if (strcmp(uris[i], uri) == 0) return (LV2_URID)(i + 1);
    }
    uris = realloc(uris, (n_uris + 1) * sizeof(char *));
    uris[n_uris] = strdup(uri);
    return (LV2_URID)(++n_uris);
}

static int fail(const char *what, const char *detail) {
    fprintf(stderr, "lv2_smoke: %s%s%s\n", what, detail ? ": " : "", detail ? detail : "");
    return 1;
}

int main(int argc, char **argv) {
    if (argc < 2 || argc > 3 || (argc == 3 && strcmp(argv[2], "--passthrough") != 0)) {
        fprintf(stderr, "usage: lv2_smoke <bundle.lv2> [--passthrough]\n");
        return 2;
    }
    int passthrough = argc == 3;

    // lilv wants the bundle as an absolute file URI with a trailing slash.
    char abs[PATH_MAX];
    if (!realpath(argv[1], abs)) return fail("no such bundle", argv[1]);
    size_t len = strlen(abs);
    if (len + 1 >= sizeof abs) return fail("bundle path too long", argv[1]);
    if (abs[len - 1] != '/') strcat(abs, "/");

    LilvWorld *world = lilv_world_new();
    LilvNode *bundle = lilv_new_file_uri(world, NULL, abs);
    lilv_world_load_bundle(world, bundle);

    const LilvPlugin *plugin = NULL;
    int found = 0;
    const LilvPlugins *all = lilv_world_get_all_plugins(world);
    LILV_FOREACH(plugins, it, all) {
        const LilvPlugin *p = lilv_plugins_get(all, it);
        if (lilv_node_equals(lilv_plugin_get_bundle_uri(p), bundle)) {
            plugin = p;
            found++;
        }
    }
    if (found != 1) {
        char n[32];
        snprintf(n, sizeof n, "%d plugins in the bundle, expected 1", found);
        return fail(n, lilv_node_as_uri(bundle));
    }
    const char *uri = lilv_node_as_uri(lilv_plugin_get_uri(plugin));
    if (!lilv_plugin_verify(plugin)) return fail("lilv_plugin_verify rejected", uri);

    LV2_URID_Map map = {NULL, map_uri};
    LV2_Feature map_feature = {LV2_URID__map, &map};
    const LV2_Feature *features[] = {&map_feature, NULL};
    LilvInstance *instance = lilv_plugin_instantiate(plugin, 48000.0, features);
    if (!instance) return fail("instantiate failed", uri);

    LilvNode *audio = lilv_new_uri(world, LV2_CORE__AudioPort);
    LilvNode *control = lilv_new_uri(world, LV2_CORE__ControlPort);
    LilvNode *atom = lilv_new_uri(world, LV2_ATOM__AtomPort);
    LilvNode *input = lilv_new_uri(world, LV2_CORE__InputPort);

    uint32_t n_ports = lilv_plugin_get_num_ports(plugin);
    float *mins = calloc(n_ports, sizeof(float));
    float *defaults = calloc(n_ports, sizeof(float));
    lilv_plugin_get_port_ranges_float(plugin, mins, NULL, defaults);
    float *controls = calloc(n_ports, sizeof(float));
    float **audio_bufs = calloc(n_ports, sizeof(float *));
    void **atom_bufs = calloc(n_ports, sizeof(void *));
    int *audio_in_index = calloc(n_ports, sizeof(int));
    int n_audio_in = 0, n_audio_out = 0, n_control = 0, n_atom = 0;
    LV2_URID sequence = map_uri(NULL, LV2_ATOM__Sequence);

    for (uint32_t i = 0; i < n_ports; i++) {
        const LilvPort *port = lilv_plugin_get_port_by_index(plugin, i);
        int is_input = lilv_port_is_a(plugin, port, input);
        if (lilv_port_is_a(plugin, port, audio)) {
            audio_bufs[i] = calloc(BLOCK, sizeof(float));
            if (is_input) {
                audio_in_index[i] = n_audio_in++;
                for (int s = 0; s < BLOCK; s++) audio_bufs[i][s] = 0.1f * (float)audio_in_index[i] + 0.1f;
            } else {
                audio_in_index[i] = n_audio_out++;
            }
            lilv_instance_connect_port(instance, i, audio_bufs[i]);
        } else if (lilv_port_is_a(plugin, port, control)) {
            controls[i] = !isnan(defaults[i]) ? defaults[i] : !isnan(mins[i]) ? mins[i] : 0.0f;
            lilv_instance_connect_port(instance, i, &controls[i]);
            n_control++;
        } else if (lilv_port_is_a(plugin, port, atom)) {
            LV2_Atom_Sequence *seq = calloc(1, ATOM_CAPACITY);
            seq->atom.type = sequence;
            seq->atom.size = is_input ? (uint32_t)sizeof(LV2_Atom_Sequence_Body)
                                      : (uint32_t)(ATOM_CAPACITY - sizeof(LV2_Atom));
            atom_bufs[i] = seq;
            lilv_instance_connect_port(instance, i, seq);
            n_atom++;
        } else {
            return fail("port of a type this smoke test doesn't connect",
                        lilv_node_as_string(lilv_port_get_symbol(plugin, port)));
        }
    }

    lilv_instance_activate(instance);
    for (int block = 0; block < 2; block++) {
        // A plugin may write its output atom ports; give them room again.
        for (uint32_t i = 0; i < n_ports; i++) {
            const LilvPort *port = lilv_plugin_get_port_by_index(plugin, i);
            if (atom_bufs[i] && !lilv_port_is_a(plugin, port, input)) {
                ((LV2_Atom_Sequence *)atom_bufs[i])->atom.size =
                    (uint32_t)(ATOM_CAPACITY - sizeof(LV2_Atom));
            }
        }
        lilv_instance_run(instance, BLOCK);
    }
    lilv_instance_deactivate(instance);

    int status = 0;
    for (uint32_t i = 0; i < n_ports; i++) {
        const LilvPort *port = lilv_plugin_get_port_by_index(plugin, i);
        if (!audio_bufs[i] || lilv_port_is_a(plugin, port, input)) continue;
        const char *symbol = lilv_node_as_string(lilv_port_get_symbol(plugin, port));
        float expected = 0.1f * (float)audio_in_index[i] + 0.1f;
        for (int s = 0; s < BLOCK; s++) {
            float v = audio_bufs[i][s];
            if (!isfinite(v)) {
                status = fail("non-finite audio output", symbol);
                break;
            }
            if (passthrough && audio_in_index[i] < n_audio_in && fabsf(v - expected) > 1e-6f) {
                fprintf(stderr, "lv2_smoke: %s sample %d is %g, expected the input's %g\n", symbol, s,
                        (double)v, (double)expected);
                status = 1;
                break;
            }
        }
    }

    lilv_instance_free(instance);
    if (status == 0) {
        printf("ok %s: %d audio in, %d audio out, %d control, %d atom ports\n", uri, n_audio_in,
               n_audio_out, n_control, n_atom);
    }
    lilv_world_free(world);
    return status;
}
