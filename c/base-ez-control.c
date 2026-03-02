#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <unistd.h>
#include <time.h>
#include <pthread.h>
#include <sys/socket.h>
#include <arpa/inet.h>
#include <errno.h>
#include <stdatomic.h>

// 引入第三方库
#include "ikcp.h"
#include "mongoose.h"
#include "pb_encode.h"
#include "pb_decode.h"
#include "public_api_types.pb.h"
#include "public_api_up.pb.h"
#include "public_api_down.pb.h"

// ==========================================
// 0. 全局变量与锁
// ==========================================
static pthread_mutex_t g_kcp_mutex = PTHREAD_MUTEX_INITIALIZER;
static atomic_bool g_running = true;
static atomic_uint_fast64_t g_session_id = 0;
static atomic_uint_fast32_t g_kcp_server_port = 0;
static atomic_bool g_drain_mode = false;

// 拼包缓冲区
#define RECV_BUF_SIZE 4096
static uint8_t g_stream_buf[RECV_BUF_SIZE];
static size_t g_stream_len = 0;

// ==========================================
// 1. 辅助工具
// ==========================================
static uint32_t current_ms() {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint32_t)(ts.tv_sec * 1000 + ts.tv_nsec / 1000000);
}

static uint32_t read_uint32_le(const uint8_t* buf) {
    return (uint32_t)buf[0] | ((uint32_t)buf[1] << 8) | 
           ((uint32_t)buf[2] << 16) | ((uint32_t)buf[3] << 24);
}

// 构造 C-Style Header [0x82, 0x00, Len_L, Len_H]
static void make_header(uint8_t* hdr, uint16_t len) {
    hdr[0] = 0x82; // Opcode 2
    hdr[1] = 0x00;
    hdr[2] = (uint8_t)(len & 0xFF);
    hdr[3] = (uint8_t)((len >> 8) & 0xFF);
}

// ==========================================
// 2. 网络上下文结构
// ==========================================
typedef struct {
    int udp_fd;
    struct sockaddr_in remote_addr;
    ikcpcb* kcp;
} KcpContext;

static int udp_output(const char *buf, int len, ikcpcb *kcp, void *user) {
    KcpContext* ctx = (KcpContext*)user;
    sendto(ctx->udp_fd, buf, len, 0, (struct sockaddr*)&ctx->remote_addr, sizeof(ctx->remote_addr));
    return 0;
}

// ==========================================
// 3. 业务逻辑：处理接收到的数据 (含拼包)
// ==========================================
static void process_payload(const uint8_t* data, size_t len) {
    // 追加数据到缓冲区
    if (g_stream_len + len > RECV_BUF_SIZE) {
        printf("[Err] Buffer overflow, resetting\n");
        g_stream_len = 0;
        return;
    }
    memcpy(g_stream_buf + g_stream_len, data, len);
    g_stream_len += len;

    while (1) {
        if (g_stream_len < 4) break; // 数据不足头长度

        uint8_t b0 = g_stream_buf[0];
        uint32_t payload_len = 0;
        size_t header_len = 0;
        int is_proto = 0;

        // 探测头格式
        if (b0 >= 0x80) { // C-Style [0x82, 0x00, Len_L, Len_H]
            payload_len = (uint16_t)((uint8_t)g_stream_buf[2] | ((uint8_t)g_stream_buf[3] << 8));
            header_len = 4;
            is_proto = 1;
        } else if (b0 <= 5) { // Rust-Style
            if (g_stream_len < 5) break; 
            payload_len = read_uint32_le(g_stream_buf + 1);
            header_len = 5;
            is_proto = 1;
        } else {
            // 未知数据，移除1字节尝试重同步
            memmove(g_stream_buf, g_stream_buf + 1, g_stream_len - 1);
            g_stream_len--;
            continue;
        }

        // 检查包是否完整
        if (g_stream_len < header_len + payload_len) break; // 等待更多数据

        // 解析 Protobuf
        if (is_proto) {
            APIUp msg = APIUp_init_zero;
            pb_istream_t stream = pb_istream_from_buffer(g_stream_buf + header_len, payload_len);
            
            if (pb_decode(&stream, APIUp_fields, &msg)) {
                if (msg.which_status == APIUp_base_status_tag) {
                    if (msg.status.base_status.has_estimated_odometry) {
                        BaseEstimatedOdometry* odom = &msg.status.base_status.estimated_odometry;
                        // printf("[Info] Odom:%s",msg.status.base_status.estimated_odometry);
                        printf("[Info] Odom: x=%.3f, y=%.3f, z=%.3f\n", 
                               odom->speed_x, odom->speed_y, odom->speed_z);
                    } else {
                        // printf("[Status] State: %d\n", msg.status.base_status.state);
                    }
                }
            }
        }

        // 移除已处理的包
        size_t total_consumed = header_len + payload_len;
        memmove(g_stream_buf, g_stream_buf + total_consumed, g_stream_len - total_consumed);
        g_stream_len -= total_consumed;
    }
}

// ==========================================
// 4. 后台线程：KCP Update 和 UDP Recv
// ==========================================
static void* kcp_worker_thread(void* arg) {
    KcpContext* ctx = (KcpContext*)arg;
    uint8_t buf[2048];

    while (g_running) {
        // 1. KCP Update (加锁)
        uint32_t now = current_ms();
        pthread_mutex_lock(&g_kcp_mutex);
        if (ctx->kcp) {
            ikcp_update(ctx->kcp, now);
        }
        pthread_mutex_unlock(&g_kcp_mutex);

        // 2. 尝试接收 UDP 数据 (非阻塞或超时)
        // 为了简单，这里使用 select 设置超时来控制 update 频率 (5ms)
        fd_set fds;
        FD_ZERO(&fds);
        FD_SET(ctx->udp_fd, &fds);
        struct timeval tv = {0, 5000}; // 5ms

        int ret = select(ctx->udp_fd + 1, &fds, NULL, NULL, &tv);
        if (ret > 0 && FD_ISSET(ctx->udp_fd, &fds)) {
            ssize_t n = recvfrom(ctx->udp_fd, buf, sizeof(buf), 0, NULL, NULL);
            if (n > 0) {
                pthread_mutex_lock(&g_kcp_mutex);
                if (ctx->kcp) {
                    ikcp_input(ctx->kcp, (char*)buf, n);
                    // 循环读取 KCP 组装好的数据
                    while (1) {
                        int peek_size = ikcp_peeksize(ctx->kcp);
                        if (peek_size <= 0) break;
                        
                        uint8_t* rx_buf = malloc(peek_size);
                        int recv_len = ikcp_recv(ctx->kcp, (char*)rx_buf, peek_size);
                        if (recv_len > 0) {
                            process_payload(rx_buf, recv_len);
                        }
                        free(rx_buf);
                    }
                }
                pthread_mutex_unlock(&g_kcp_mutex);
            }
        }
    }
    return NULL;
}

// 辅助：通过 KCP 发送 Protobuf
static void kcp_send_proto(KcpContext* ctx, const APIDown* msg) {
    uint8_t payload[1024];
    pb_ostream_t stream = pb_ostream_from_buffer(payload, sizeof(payload));
    
    if (!pb_encode(&stream, APIDown_fields, msg)) return;
    
    size_t len = stream.bytes_written;
    uint8_t header[4];
    make_header(header, (uint16_t)len);

    // 拼接 Header + Payload
    uint8_t* frame = malloc(4 + len);
    memcpy(frame, header, 4);
    memcpy(frame + 4, payload, len);

    pthread_mutex_lock(&g_kcp_mutex);
    if (ctx->kcp && g_running) {
        ikcp_send(ctx->kcp, (char*)frame, 4 + len);
        ikcp_flush(ctx->kcp);
    }
    pthread_mutex_unlock(&g_kcp_mutex);
    free(frame);
}

// 辅助：通过 WebSocket 发送 Protobuf
static void ws_send_proto(struct mg_connection *c, const APIDown* msg) {
    uint8_t payload[1024];
    pb_ostream_t stream = pb_ostream_from_buffer(payload, sizeof(payload));
    if (pb_encode(&stream, APIDown_fields, msg)) {
        mg_ws_send(c, payload, stream.bytes_written, WEBSOCKET_OP_BINARY);
    }
}

// ==========================================
// 5. WebSocket 事件回调
// ==========================================
static void ws_event_handler(struct mg_connection *c, int ev, void *ev_data) {
    if (ev == MG_EV_WS_MSG) {
        if (g_drain_mode) return; // Drain Mode

        struct mg_ws_message *wm = (struct mg_ws_message *)ev_data;
        if (wm->flags & WEBSOCKET_OP_BINARY) {
            APIUp msg = APIUp_init_zero;
            pb_istream_t stream = pb_istream_from_buffer(wm->data.buf, wm->data.len);
            if (pb_decode(&stream, APIUp_fields, &msg)) {
                // 提取 Session ID
                if (g_session_id == 0 && msg.session_id != 0) {
                    g_session_id = msg.session_id;
                    // printf("[Step 1] Got Session ID: %lu\n", g_session_id);
                }
                // 提取 KCP 端口
                if (msg.has_kcp_server_status) {
                    int port = msg.kcp_server_status.server_port;
                    if (port != 0 && g_kcp_server_port == 0) {
                        g_kcp_server_port = port;
                        // printf("[Step 4] Got KCP Port: %d\n", g_kcp_server_port);
                    }
                }
            }
        }
    }
}

// ==========================================
// 6. 主函数
// ==========================================
int main(int argc, char* argv[]) {
    // 处理参数
    const char* ip = (argc >= 2) ? argv[1] : "127.0.0.1";
    char url[128];
    snprintf(url, sizeof(url), "ws://%s:8439", ip);
    
    printf("[Sys] Connecting to: %s\n", url);

    // 1. 初始化 Mongoose (WebSocket)
    struct mg_mgr mgr;
    mg_mgr_init(&mgr);
    struct mg_connection *c = mg_ws_connect(&mgr, url, ws_event_handler, NULL, NULL);
    if (!c) {
        printf("[Err] WS Connect Failed\n");
        return 1;
    }

    // 2. 循环等待 Session ID
    while (g_session_id == 0 && g_running) mg_mgr_poll(&mgr, 10);
    printf("[Step 1] Session ID: %lu\n", g_session_id);

    // 3. 初始化 UDP
    int udp_fd = socket(AF_INET, SOCK_DGRAM, 0);
    struct sockaddr_in local_addr = {0};
    local_addr.sin_family = AF_INET;
    local_addr.sin_addr.s_addr = htonl(INADDR_ANY);
    local_addr.sin_port = 0; // 自动分配
    bind(udp_fd, (struct sockaddr*)&local_addr, sizeof(local_addr));
    
    socklen_t addr_len = sizeof(local_addr);
    getsockname(udp_fd, (struct sockaddr*)&local_addr, &addr_len);
    uint16_t local_port = ntohs(local_addr.sin_port);
    printf("[Step 2] Local UDP Port: %d\n", local_port);

    // 4. 通过 WS 发送 EnableKCP
    {
        APIDown msg = APIDown_init_zero;
        msg.which_down = APIDown_enable_kcp_tag;
        msg.down.enable_kcp.client_peer_port = local_port;
        msg.down.enable_kcp.has_kcp_config = true;
        msg.down.enable_kcp.kcp_config.window_size_snd_wnd = 128;
        msg.down.enable_kcp.kcp_config.window_size_rcv_wnd = 128;
        msg.down.enable_kcp.kcp_config.interval_ms = 10;
        msg.down.enable_kcp.kcp_config.no_delay = true;
        msg.down.enable_kcp.kcp_config.nc = true;
        msg.down.enable_kcp.kcp_config.resend = 2;
        ws_send_proto(c, &msg);
    }

    // 5. 等待 KCP Server Port
    while (g_kcp_server_port == 0 && g_running) mg_mgr_poll(&mgr, 10);
    printf("[Step 4] Server KCP Port: %d\n", g_kcp_server_port);

    // 6. 初始化 KCP 上下文
    KcpContext kcp_ctx;
    kcp_ctx.udp_fd = udp_fd;
    kcp_ctx.remote_addr.sin_family = AF_INET;
    kcp_ctx.remote_addr.sin_port = htons(g_kcp_server_port);
    inet_pton(AF_INET, ip, &kcp_ctx.remote_addr.sin_addr);

    kcp_ctx.kcp = ikcp_create((uint32_t)g_session_id, &kcp_ctx);
    kcp_ctx.kcp->output = udp_output;
    ikcp_nodelay(kcp_ctx.kcp, 1, 10, 2, 1);
    ikcp_wndsize(kcp_ctx.kcp, 128, 128);
    ikcp_setmtu(kcp_ctx.kcp, 1400);

    // 7. 启动后台线程 (Update & Recv)
    pthread_t thread_id;
    pthread_create(&thread_id, NULL, kcp_worker_thread, &kcp_ctx);

    // 8. 激活 KCP
    {
        APIDown msg = APIDown_init_zero;
        msg.which_down = APIDown_placeholder_message_tag;
        msg.down.placeholder_message = true;
        kcp_send_proto(&kcp_ctx, &msg);
        printf("[Step 6] KCP Activated\n");
    }

    // 9. WS 降频 & 清除停车
    {
        APIDown msg1 = APIDown_init_zero;
        msg1.which_down = APIDown_set_report_frequency_tag;
        msg1.down.set_report_frequency = ReportFrequency_Rf1Hz;
        ws_send_proto(c, &msg1);

        APIDown msg2 = APIDown_init_zero;
        msg2.which_down = APIDown_base_command_tag;
        msg2.down.base_command.which_command = BaseCommand_clear_parking_stop_tag;
        msg2.down.base_command.command.clear_parking_stop = true;
        ws_send_proto(c, &msg2);
    }

    g_drain_mode = true; // 忽略后续 WS 消息

    // 10. KCP 升频 & Init Base
    {
        APIDown msg1 = APIDown_init_zero;
        msg1.which_down = APIDown_set_report_frequency_tag;
        msg1.down.set_report_frequency = ReportFrequency_Rf250Hz;
        kcp_send_proto(&kcp_ctx, &msg1);

        APIDown msg2 = APIDown_init_zero;
        msg2.which_down = APIDown_base_command_tag;
        msg2.down.base_command.which_command = BaseCommand_api_control_initialize_tag;
        msg2.down.base_command.command.api_control_initialize = true;
        kcp_send_proto(&kcp_ctx, &msg2);
        printf("[Step 11] Base Initialized\n");
    }

    // 11. 主循环
    printf("[Step 12] Start Moving Loop (10s)...\n");
    uint32_t start_time = current_ms();
    uint32_t last_ws_hb = 0;

    APIDown move_msg = APIDown_init_zero;
    move_msg.which_down = APIDown_base_command_tag;
    move_msg.down.base_command.which_command = BaseCommand_simple_move_command_tag;
    move_msg.down.base_command.command.simple_move_command.which_command = SimpleBaseMoveCommand_xyz_speed_tag;
    move_msg.down.base_command.command.simple_move_command.command.xyz_speed.speed_x = 0.1;

    APIDown hb_msg = APIDown_init_zero;
    hb_msg.which_down = APIDown_placeholder_message_tag;
    hb_msg.down.placeholder_message = true;

    while ((current_ms() - start_time) < 1000*60*10) {
        // A. KCP 移动指令
        kcp_send_proto(&kcp_ctx, &move_msg);

        // B. WS 心跳 (1Hz)
        uint32_t now = current_ms();
        if (now - last_ws_hb >= 1000) {
            ws_send_proto(c, &hb_msg);
            last_ws_hb = now;
        }

        // C. Mongoose Poll (保持 WS 连接活跃)
        mg_mgr_poll(&mgr, 0);

        usleep(4000); // 4ms
    }

    // 12. Deinit
    {
        APIDown msg = APIDown_init_zero;
        msg.which_down = APIDown_base_command_tag;
        msg.down.base_command.which_command = BaseCommand_api_control_initialize_tag;
        msg.down.base_command.command.api_control_initialize = false;
        ws_send_proto(c, &msg);
        printf("[Info] Successfully deinitialized base\n");
    }

    // 13. 优雅退出
    g_running = false; // 通知线程停止
    pthread_join(thread_id, NULL); // 等待线程结束

    // 销毁资源
    if (kcp_ctx.kcp) ikcp_release(kcp_ctx.kcp);
    close(udp_fd);
    mg_mgr_free(&mgr);
    
    printf("[Sys] Application Exit\n");
    return 0;
}