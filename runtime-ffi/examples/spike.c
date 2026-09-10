/* ABI spike C host: handshake -> honesty check -> demo stream -> pull events
 * to terminal -> WOULD_BLOCK branch -> cancel API -> teardown.
 * Verifies: ABI handshake, NOT_CONFIGURED honesty (no silent fake data),
 * runtime_status, pull-based stream, ownership (string_free), NULL-safety,
 * terminal-event guarantee.
 *
 * 真实调用（连真 Provider）不用 demo：注册部署 + 写凭据即可，
 * 见 runtime-ffi/include/umer.h 的 runtime_set_deployment / 
 * runtime_set_credential / runtime_load_catalog。
 */
#include <stdio.h>
#include <string.h>
#include "umer.h"

static const char* REQUEST_JSON =
    "{\"model\":\"demo\",\"messages\":[{\"role\":\"user\","
    "\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}]}";

int main(void) {
    /* 1. ABI version handshake */
    uint32_t version = runtime_abi_version();
    uint32_t major = version >> 16;
    printf("[spike] abi version %u.%u\n", major, version & 0xFFFF);
    if (major != 0) {
        printf("[spike] FAIL: unexpected abi major\n");
        return 1;
    }

    /* 2. init */
    UmerRuntime* rt = runtime_init();
    if (!rt) {
        printf("[spike] FAIL: runtime_init returned NULL\n");
        return 1;
    }

    UmerEvent ev;
    int32_t status;

    /* 3. honesty check: nothing configured and demo OFF (the default) must
     *    fail loudly instead of silently returning built-in fake data */
    UmerStream* refused = NULL;
    status = runtime_stream_open(rt, REQUEST_JSON, strlen(REQUEST_JSON), &refused);
    if (status != UMER_ERR_NOT_CONFIGURED) {
        printf("[spike] FAIL: expected NOT_CONFIGURED, got %d\n", status);
        runtime_shutdown(rt);
        return 1;
    }
    printf("[spike] unconfigured open refused with NOT_CONFIGURED (no fake data)\n");

    /* 4. explicit opt-in to the built-in demo source (fake events, no network) */
    if (runtime_set_demo(rt, 1) != UMER_OK) {
        printf("[spike] FAIL: runtime_set_demo\n");
        runtime_shutdown(rt);
        return 1;
    }

    /* 5. runtime_status: host self-check of what is configured */
    memset(&ev, 0, sizeof(ev));
    if (runtime_status(rt, &ev) != UMER_EVENT) {
        printf("[spike] FAIL: runtime_status\n");
        runtime_shutdown(rt);
        return 1;
    }
    printf("[spike] status %.*s\n", (int)ev.json_len, ev.json);
    runtime_string_free((char*)ev.json);

    /* 6. open stream */
    UmerStream* stream = NULL;
    status = runtime_stream_open(rt, REQUEST_JSON, strlen(REQUEST_JSON), &stream);
    if (status != UMER_OK || !stream) {
        printf("[spike] FAIL: stream_open status=%d\n", status);
        runtime_shutdown(rt);
        return 1;
    }

    /* 4. NULL safety: cancel(NULL) must return an error code, not crash */
    if (runtime_stream_cancel(NULL) != UMER_ERR_NULL_ARGUMENT) {
        printf("[spike] FAIL: null cancel not rejected\n");
        runtime_stream_close(stream);
        runtime_shutdown(rt);
        return 1;
    }

    /* 7. deterministic WOULD_BLOCK: engine synthesizes Started instantly, but
     *    the demo source throttles the first provider event by 300ms, so a
     *    5ms pull right after Started must come back WOULD_BLOCK */
    memset(&ev, 0, sizeof(ev));
    status = runtime_stream_next(stream, 2000, &ev); /* consumes Started */
    if (status != UMER_EVENT) {
        printf("[spike] FAIL: expected Started event, got %d\n", status);
        runtime_stream_close(stream);
        runtime_shutdown(rt);
        return 1;
    }
    printf("[spike] seq=%llu (Started delivered instantly)\n",
           (unsigned long long)ev.sequence);
    runtime_string_free((char*)ev.json);

    memset(&ev, 0, sizeof(ev));
    status = runtime_stream_next(stream, 5, &ev);
    if (status != UMER_WOULD_BLOCK) {
        printf("[spike] FAIL: expected WOULD_BLOCK on short pull, got %d\n", status);
        runtime_stream_close(stream);
        runtime_shutdown(rt);
        return 1;
    }
    printf("[spike] short pull returned WOULD_BLOCK as designed\n");

    /* 6. pull events until terminal */
    int events = 0;
    int terminals = 0;
    for (;;) {
        memset(&ev, 0, sizeof(ev));
        status = runtime_stream_next(stream, 2000, &ev);
        if (status == UMER_EVENT) {
            events++;
            size_t show = ev.json_len < 96 ? ev.json_len : 96;
            printf("[spike] seq=%llu %.*s%s\n",
                   (unsigned long long)ev.sequence,
                   (int)show, ev.json,
                   show < ev.json_len ? " ..." : "");
            if (strstr(ev.json, "\"completed\"") || strstr(ev.json, "\"failed\"") ||
                strstr(ev.json, "\"cancelled\"")) {
                terminals++;
            }
            runtime_string_free((char*)ev.json);
        } else if (status == UMER_CLOSED) {
            break;
        } else {
            printf("[spike] FAIL: next status=%d\n", status);
            runtime_stream_close(stream);
            runtime_shutdown(rt);
            return 1;
        }
    }

    /* 7. pulling from a closed stream returns CLOSED (not an error, no crash) */
    memset(&ev, 0, sizeof(ev));
    if (runtime_stream_next(stream, 100, &ev) != UMER_CLOSED) {
        printf("[spike] FAIL: closed stream did not return CLOSED\n");
        runtime_stream_close(stream);
        runtime_shutdown(rt);
        return 1;
    }

    /* 8. teardown */
    runtime_stream_cancel(stream);
    runtime_stream_close(stream);
    runtime_shutdown(rt);

    printf("[spike] events=%d terminals=%d\n", events, terminals);
    if (events < 5 || terminals != 1) {
        printf("[spike] FAIL: unexpected event statistics\n");
        return 1;
    }
    printf("[spike] PASS\n");
    return 0;
}
