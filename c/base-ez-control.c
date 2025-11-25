#include <sys/time.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <arpa/inet.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <pthread.h>
#include <errno.h>
#include "mongoose.h"
#include "pb_decode.h"
#include "pb_encode.h"
#include "generated/inc/public_api_up.pb.h"
#include "generated/inc/public_api_down.pb.h"
#include "ikcp.h"

#define EXPECTED_PROTOCOL_MAJOR_VERSION 1

static volatile bool quit = false;

/* State filled by websocket handler */
static volatile uint32_t g_session_id = 0;
static volatile bool g_got_session = false;
static volatile bool g_got_kcp_server = false;
static volatile uint32_t g_kcp_server_port = 0;
static volatile int32_t g_kcp_snd_wnd = 0;
static volatile int32_t g_kcp_rcv_wnd = 0;
static volatile int32_t g_kcp_interval = 0;
static volatile bool g_kcp_no_delay = false;
static volatile bool g_kcp_nc = false;
static volatile int32_t g_kcp_resend = 0;

/* Helper to get current time in milliseconds */
static uint32_t now_ms()
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint32_t)(ts.tv_sec * 1000 + ts.tv_nsec / 1000000);
}

/* Websocket handler: decode APIUp and extract session_id and KCP server info */
static void ws_handler(struct mg_connection *c, int ev, void *ev_data)
{
    switch (ev)
    {
    case MG_EV_WS_OPEN:
    {
        // Set initial report frequency to 50Hz to avoid excessive messages on websocket
        APIDown change_frequency_msg = APIDown_init_default;
        change_frequency_msg.which_down = APIDown_set_report_frequency_tag;
        change_frequency_msg.down.set_report_frequency = ReportFrequency_Rf50Hz;
        uint8_t obuffer[1024];
        pb_ostream_t stream = pb_ostream_from_buffer(obuffer, sizeof(obuffer));
        bool status = pb_encode(&stream, APIDown_fields, &change_frequency_msg);
        size_t change_frequency_buffer_size = stream.bytes_written;
        if (!status)
        {
            printf("Failed to encode APIDown message\n");
            c->is_closing = 1;
            quit = true;
            return;
        }
        mg_ws_send(c, obuffer, change_frequency_buffer_size, WEBSOCKET_OP_BINARY);
        break;
    }
    case MG_EV_WS_MSG:
    {
        struct mg_ws_message *wm = (struct mg_ws_message *)ev_data;
        if (!(wm->flags & WEBSOCKET_OP_BINARY))
        {
            // Ignore non-binary
            break;
        }
        pb_istream_t stream = pb_istream_from_buffer(wm->data.buf, wm->data.len);
        APIUp rx_msg = APIUp_init_zero;
        bool status = pb_decode(&stream, APIUp_fields, &rx_msg);
        if (!status)
        {
            printf("Failed to decode APIUp message\n");
            c->is_closing = 1;
            quit = true;
            return;
        }

        /* Basic protocol version check */
        if (rx_msg.protocol_major_version != EXPECTED_PROTOCOL_MAJOR_VERSION)
        {
            printf("Protocol major version is not %d, current version: %d. This might cause compatibility issues. Consider upgrading the base firmware.\n", EXPECTED_PROTOCOL_MAJOR_VERSION, rx_msg.protocol_major_version);
            /* We don't immediately quit here: just warn and stop decoding odometry */
        }

        /* Save session id if present */
        if (rx_msg.session_id != 0 && !g_got_session)
        {
            g_session_id = rx_msg.session_id;
            g_got_session = true;
            printf("Got session id: %u\n", g_session_id);
        }

        /* If KCP server info present, store it */
        if (rx_msg.has_kcp_server_status && !g_got_kcp_server)
        {
            g_kcp_server_port = rx_msg.kcp_server_status.server_port;
            g_got_kcp_server = true;
            if (rx_msg.kcp_server_status.has_kcp_config)
            {
                g_kcp_snd_wnd = rx_msg.kcp_server_status.kcp_config.window_size_snd_wnd;
                g_kcp_rcv_wnd = rx_msg.kcp_server_status.kcp_config.window_size_rcv_wnd;
                g_kcp_interval = rx_msg.kcp_server_status.kcp_config.interval_ms;
                g_kcp_no_delay = rx_msg.kcp_server_status.kcp_config.no_delay;
                g_kcp_nc = rx_msg.kcp_server_status.kcp_config.nc;
                g_kcp_resend = rx_msg.kcp_server_status.kcp_config.resend;
            }
            printf("Got KCP server port: %u\n", g_kcp_server_port);
        }

        /* Print base odometry if available */
        if (rx_msg.which_status == APIUp_base_status_tag)
        {
            BaseStatus bs = rx_msg.status.base_status;
            if (bs.has_estimated_odometry)
            {
                printf("Estimated odometry (ws): spd_x=%.3f spd_y=%.3f spd_z=%.3f\n",
                       bs.estimated_odometry.speed_x,
                       bs.estimated_odometry.speed_y,
                       bs.estimated_odometry.speed_z);
            }
        }
        break;
    }
    case MG_EV_ERROR:
    {
        c->is_closing = 1;
        break;
    }
    default:
        break;
    }
}

/* UDP + KCP helper structures and functions */
struct udp_user {
    int fd;
    struct sockaddr_in server;
};

static int kcp_udp_output(const char *buf, int len, struct IKCPCB *kcp, void *user)
{
    struct udp_user *u = (struct udp_user *)user;
    ssize_t r = sendto(u->fd, buf, len, 0, (struct sockaddr *)&u->server, sizeof(u->server));
    if (r < 0)
    {
        perror("sendto");
        return -1;
    }
    return 0;
}

/* Parse hex-socket framed data: header 4 bytes, byte0 = 0x80 | opcode, byte1=0, bytes2-3 little endian length */
static void handle_app_message(const uint8_t *data, size_t len)
{
    size_t pos = 0;
    while (pos + 4 <= len)
    {
        uint8_t b0 = data[pos];
        uint8_t opcode = b0 & 0x0F;
        uint16_t mlen = (uint16_t)(data[pos + 2] | (data[pos + 3] << 8));
        if (pos + 4 + mlen > len)
            break; /* incomplete */
        if ((b0 & 0x80) && opcode == 0x2)
        {
            /* Binary payload */
            pb_istream_t stream = pb_istream_from_buffer((const pb_byte_t *)&data[pos + 4], mlen);
            APIUp msg = APIUp_init_zero;
            bool ok = pb_decode(&stream, APIUp_fields, &msg);
            if (!ok)
            {
                printf("Failed to decode APIUp from KCP\n");
            }
            else
            {
                if (msg.which_status == APIUp_base_status_tag)
                {
                    BaseStatus bs = msg.status.base_status;
                    if (bs.has_estimated_odometry)
                    {
                        printf("Estimated odometry (kcp): spd_x=%.3f spd_y=%.3f spd_z=%.3f\n",
                               bs.estimated_odometry.speed_x,
                               bs.estimated_odometry.speed_y,
                               bs.estimated_odometry.speed_z);
                    }
                }
            }
        }
        pos += 4 + mlen;
    }
}

struct kcp_thread_ctx {
    ikcpcb *kcp;
    struct udp_user *u;
};

/* Thread: receive UDP, feed to KCP, extract application messages */
static void *kcp_recv_thread(void *arg)
{
    struct kcp_thread_ctx *ctx = (struct kcp_thread_ctx *)arg;
    int fd = ctx->u->fd;
    ikcpcb *kcp = ctx->kcp;
    uint8_t buf[4096];
    while (!quit)
    {
        struct sockaddr_in from;
        socklen_t fromlen = sizeof(from);
        ssize_t n = recvfrom(fd, buf, sizeof(buf), 0, (struct sockaddr *)&from, &fromlen);
        if (n < 0)
        {
            if (errno == EINTR)
                continue;
            perror("recvfrom");
            break;
        }
        if (n > 0)
        {
            /* Feed into KCP */
            ikcp_input(kcp, (const char *)buf, (long)n);
            /* Try to receive application messages */
            while (1)
            {
                int peek = ikcp_peeksize(kcp);
                if (peek <= 0)
                    break;
                if (peek > (int)sizeof(buf))
                {
                    printf("Message too large: %d\n", peek);
                    /* drain */
                    char *tmp = malloc(peek);
                    if (!tmp) break;
                    ikcp_recv(kcp, tmp, peek);
                    free(tmp);
                    continue;
                }
                int rr = ikcp_recv(kcp, (char *)buf, sizeof(buf));
                if (rr > 0)
                {
                    handle_app_message(buf, (size_t)rr);
                }
            }
        }

        /* update kcp state periodically */
        ikcpcb *k = kcp;
        uint32_t current = now_ms();
        ikcp_update(k, current);
    }
    return NULL;
}

/* Helper to create hex-socket header */
static void make_hex_header(uint8_t *hdr, uint16_t len, uint8_t opcode)
{
    hdr[0] = 0x80 | (opcode & 0x0F);
    hdr[1] = 0x00;
    hdr[2] = (uint8_t)(len & 0xFF);
    hdr[3] = (uint8_t)((len >> 8) & 0xFF);
}

int main(int argc, char *argv[])
{
    const char *url = argc > 1 ? argv[1] : "ws://127.0.0.1:8439/websocket";
    /* If user passed IP:PORT as arg without ws:// prefix, allow that style too */
    if (argc > 1 && strstr(argv[1], "ws://") == NULL)
    {
        char tmp[256];
        snprintf(tmp, sizeof(tmp), "ws://%s/websocket", argv[1]);
        url = strdup(tmp);
    }

    struct mg_mgr mgr;
    mg_mgr_init(&mgr);

    mg_log_set(MG_LL_INFO);

    /* Connect websocket */
    struct mg_connection *c = mg_ws_connect(&mgr, url, ws_handler, NULL, NULL);
    if (c == NULL)
    {
        fprintf(stderr, "Failed to start WS connection to %s\n", url);
        mg_mgr_free(&mgr);
        return 1;
    }

    /* Prepare common APIDown messages */
    APIDown init_msg = APIDown_init_default;
    init_msg.which_down = APIDown_base_command_tag;
    init_msg.down.base_command.which_command = BaseCommand_api_control_initialize_tag;
    init_msg.down.base_command.command.api_control_initialize = true;
    uint8_t init_buffer[1024];
    pb_ostream_t stream = pb_ostream_from_buffer(init_buffer, sizeof(init_buffer));
    bool status = pb_encode(&stream, APIDown_fields, &init_msg);
    if (!status)
    {
        printf("Failed to encode APIDown init message\n");
        quit = true;
    }
    size_t init_buffer_size = stream.bytes_written;

    APIDown deinit_msg = APIDown_init_default;
    deinit_msg.which_down = APIDown_base_command_tag;
    deinit_msg.down.base_command.which_command = BaseCommand_api_control_initialize_tag;
    deinit_msg.down.base_command.command.api_control_initialize = false;
    uint8_t deinit_buffer[1024];
    stream = pb_ostream_from_buffer(deinit_buffer, sizeof(deinit_buffer));
    status = pb_encode(&stream, APIDown_fields, &deinit_msg);
    if (!status)
    {
        printf("Failed to encode APIDown deinit message\n");
        quit = true;
    }
    size_t deinit_buffer_size = stream.bytes_written;

    /* Prepare move message template */
    APIDown move_msg = APIDown_init_default;
    move_msg.which_down = APIDown_base_command_tag;
    move_msg.down.base_command.which_command = BaseCommand_simple_move_command_tag;
    move_msg.down.base_command.command.simple_move_command.which_command = SimpleBaseMoveCommand_xyz_speed_tag;
    move_msg.down.base_command.command.simple_move_command.command.xyz_speed.speed_x = 0.0;
    move_msg.down.base_command.command.simple_move_command.command.xyz_speed.speed_y = 0.0;
    move_msg.down.base_command.command.simple_move_command.command.xyz_speed.speed_z = 0.1;
    uint8_t move_buffer[1024];
    stream = pb_ostream_from_buffer(move_buffer, sizeof(move_buffer));
    status = pb_encode(&stream, APIDown_fields, &move_msg);
    size_t move_buffer_size = stream.bytes_written;
    if (!status)
    {
        printf("Failed to encode APIDown move message\n");
        quit = true;
    }

    /* Wait until we receive a session id from websocket */
    while (!g_got_session && !quit)
    {
        mg_mgr_poll(&mgr, 100);
    }
    if (quit)
    {
        mg_mgr_free(&mgr);
        return 1;
    }

    /* Create UDP socket and bind to ephemeral port */
    int udpfd = socket(AF_INET, SOCK_DGRAM, 0);
    if (udpfd < 0)
    {
        perror("socket");
        mg_mgr_free(&mgr);
        return 1;
    }
    struct sockaddr_in local;
    memset(&local, 0, sizeof(local));
    local.sin_family = AF_INET;
    local.sin_addr.s_addr = htonl(INADDR_ANY);
    local.sin_port = 0; /* ephemeral */
    if (bind(udpfd, (struct sockaddr *)&local, sizeof(local)) < 0)
    {
        perror("bind");
        close(udpfd);
        mg_mgr_free(&mgr);
        return 1;
    }
    socklen_t loclen = sizeof(local);
    if (getsockname(udpfd, (struct sockaddr *)&local, &loclen) < 0)
    {
        perror("getsockname");
        close(udpfd);
        mg_mgr_free(&mgr);
        return 1;
    }
    uint16_t local_port = ntohs(local.sin_port);
    printf("Local UDP port = %u\n", local_port);

    /* Send EnableKcp via websocket, informing server of our UDP port and desired KCP config */
    APIDown enable_kcp = APIDown_init_default;
    enable_kcp.which_down = APIDown_enable_kcp_tag;
    enable_kcp.down.enable_kcp.client_peer_port = local_port;
    enable_kcp.down.enable_kcp.has_kcp_config = true;
    enable_kcp.down.enable_kcp.kcp_config.window_size_snd_wnd = 64;
    enable_kcp.down.enable_kcp.kcp_config.window_size_rcv_wnd = 64;
    enable_kcp.down.enable_kcp.kcp_config.interval_ms = 10;
    enable_kcp.down.enable_kcp.kcp_config.no_delay = true;
    enable_kcp.down.enable_kcp.kcp_config.nc = true;
    enable_kcp.down.enable_kcp.kcp_config.resend = 2;
    uint8_t enable_buffer[256];
    stream = pb_ostream_from_buffer(enable_buffer, sizeof(enable_buffer));
    status = pb_encode(&stream, APIDown_fields, &enable_kcp);
    if (!status)
    {
        printf("Failed to encode APIDown enable_kcp message\n");
    }
    size_t enable_buffer_size = stream.bytes_written;
    mg_ws_send(c, enable_buffer, enable_buffer_size, WEBSOCKET_OP_BINARY);

    /* Wait for server to respond with kcp_server_status */
    while (!g_got_kcp_server && !quit)
    {
        mg_mgr_poll(&mgr, 100);
    }
    if (quit)
    {
        close(udpfd);
        mg_mgr_free(&mgr);
        return 1;
    }

    /* Build server sockaddr */
    struct udp_user u;
    memset(&u, 0, sizeof(u));
    u.fd = udpfd;
    /* Extract host from url: expecting ws://IP:PORT/... */
    char hostbuf[128] = {0};
    {
        const char *p = strstr(url, "//");
        if (p)
            p += 2;
        else
            p = url;
        const char *col = strchr(p, ':');
        const char *slash = strchr(p, '/');
        size_t hlen = 0;
        if (col && (!slash || col < slash))
            hlen = (size_t)(col - p);
        else if (slash)
            hlen = (size_t)(slash - p);
        else
            hlen = strlen(p);
        if (hlen >= sizeof(hostbuf))
            hlen = sizeof(hostbuf) - 1;
        memcpy(hostbuf, p, hlen);
        hostbuf[hlen] = '\0';
    }
    u.server.sin_family = AF_INET;
    u.server.sin_port = htons((uint16_t)g_kcp_server_port);
    if (inet_pton(AF_INET, hostbuf, &u.server.sin_addr) <= 0)
    {
        fprintf(stderr, "Failed to parse host %s\n", hostbuf);
        close(udpfd);
        mg_mgr_free(&mgr);
        return 1;
    }

    /* Create KCP instance with conv = session_id */
    ikcpcb *kcp = ikcp_create((IUINT32)g_session_id, &u);
    if (!kcp)
    {
        fprintf(stderr, "Failed to create KCP instance\n");
        close(udpfd);
        mg_mgr_free(&mgr);
        return 1;
    }
    ikcp_setoutput(kcp, kcp_udp_output);
    /* Apply KCP config from server if present, otherwise use our suggested */
    int snd_wnd = (g_kcp_snd_wnd > 0) ? g_kcp_snd_wnd : 64;
    int rcv_wnd = (g_kcp_rcv_wnd > 0) ? g_kcp_rcv_wnd : 64;
    int interval = (g_kcp_interval > 0) ? g_kcp_interval : 10;
    bool no_delay = g_kcp_no_delay || !g_got_kcp_server;
    bool nc = g_kcp_nc || !g_got_kcp_server;
    int resend = (g_kcp_resend > 0) ? g_kcp_resend : 2;
    ikcp_wndsize(kcp, snd_wnd, rcv_wnd);
    ikcp_nodelay(kcp, no_delay ? 1 : 0, interval, resend, nc ? 1 : 0);

    /* Start recv thread */
    pthread_t th;
    struct kcp_thread_ctx ctx;
    ctx.kcp = kcp;
    ctx.u = &u;
    if (pthread_create(&th, NULL, kcp_recv_thread, &ctx) != 0)
    {
        perror("pthread_create");
        ikcp_release(kcp);
        close(udpfd);
        mg_mgr_free(&mgr);
        return 1;
    }

    /* Send a placeholder message over KCP to activate server-side KCP */
    APIDown placeholder = APIDown_init_default;
    placeholder.which_down = APIDown_placeholder_message_tag;
    placeholder.down.placeholder_message = true;
    uint8_t placeholder_buf[256];
    stream = pb_ostream_from_buffer(placeholder_buf, sizeof(placeholder_buf));
    status = pb_encode(&stream, APIDown_fields, &placeholder);
    size_t placeholder_len = stream.bytes_written;
    if (status)
    {
        uint8_t header[4];
        make_hex_header(header, (uint16_t)placeholder_len, 0x2);
        /* concatenate header + payload */
        uint8_t *sendbuf = malloc(4 + placeholder_len);
        memcpy(sendbuf, header, 4);
        memcpy(sendbuf + 4, placeholder_buf, placeholder_len);
        ikcp_send(kcp, (const char *)sendbuf, (int)(4 + placeholder_len));
        free(sendbuf);
    }

    /* Change websocket report frequency to 1Hz (optional but recommended) */
    APIDown set_rf = APIDown_init_default;
    set_rf.which_down = APIDown_set_report_frequency_tag;
    set_rf.down.set_report_frequency = ReportFrequency_Rf1Hz;
    uint8_t setrf_buf[128];
    stream = pb_ostream_from_buffer(setrf_buf, sizeof(setrf_buf));
    status = pb_encode(&stream, APIDown_fields, &set_rf);
    if (status)
    {
        mg_ws_send(c, setrf_buf, stream.bytes_written, WEBSOCKET_OP_BINARY);
    }

    /* Initialize base via KCP */
    APIDown init_kcp = APIDown_init_default;
    init_kcp.which_down = APIDown_base_command_tag;
    init_kcp.down.base_command.which_command = BaseCommand_api_control_initialize_tag;
    init_kcp.down.base_command.command.api_control_initialize = true;
    uint8_t init_kcp_buf[256];
    stream = pb_ostream_from_buffer(init_kcp_buf, sizeof(init_kcp_buf));
    status = pb_encode(&stream, APIDown_fields, &init_kcp);
    if (status)
    {
        uint8_t header[4];
        make_hex_header(header, (uint16_t)stream.bytes_written, 0x2);
        uint8_t *sendbuf = malloc(4 + stream.bytes_written);
        memcpy(sendbuf, header, 4);
        memcpy(sendbuf + 4, init_kcp_buf, stream.bytes_written);
        ikcp_send(kcp, (const char *)sendbuf, (int)(4 + stream.bytes_written));
        free(sendbuf);
    }

    /* Send move messages over KCP for 10 seconds at 50Hz (20ms) */
    uint32_t start = now_ms();
    int count = 0;
    while (!quit && (now_ms() - start) < 10000)
    {
        uint8_t header[4];
        make_hex_header(header, (uint16_t)move_buffer_size, 0x2);
        uint8_t *sendbuf = malloc(4 + move_buffer_size);
        memcpy(sendbuf, header, 4);
        memcpy(sendbuf + 4, move_buffer, move_buffer_size);
        ikcp_send(kcp, (const char *)sendbuf, (int)(4 + move_buffer_size));
        free(sendbuf);
        count++;
        /* Let mgr run so websocket messages are processed */
        mg_mgr_poll(&mgr, 1);
        usleep(20 * 1000);
    }

    printf("Sent %d KCP move messages\n", count);

    /* Send final deinit message over websocket to ensure deinitialization */
    printf("Sending deinit message over websocket\n");
    mg_ws_send(c, deinit_buffer, deinit_buffer_size, WEBSOCKET_OP_BINARY);
    while (c->send.len > 0)
    {
        mg_mgr_poll(&mgr, 1);
    }
    c->is_closing = 1;
    mg_mgr_poll(&mgr, 1);

    /* teardown */
    quit = true;
    pthread_join(th, NULL);
    ikcp_release(kcp);
    close(udpfd);
    mg_mgr_free(&mgr);
    return 0;
}
