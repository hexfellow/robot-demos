#include <iostream>
#include <thread>
#include <chrono>
#include <string>
#include <vector>
#include <mutex>
#include <atomic>
#include <condition_variable>
#include <cstring>
#include <iomanip>

// KCP Library
#include "ikcp.h"

// Networking
#include <websocketpp/config/asio_no_tls_client.hpp>
#include <websocketpp/client.hpp>
#include <boost/asio.hpp>
#include <boost/bind/bind.hpp>

// Protobuf
#include "public_api_types.pb.h"
#include "public_api_up.pb.h"
#include "public_api_down.pb.h"

using boost::asio::ip::udp;

// ==========================================
// 1. 协议辅助工具
// ==========================================
// C-Style Header: [0x82, 0x00, Len_L, Len_H]
void make_header(std::vector<uint8_t>& buf, uint16_t len) {
    buf.resize(4);
    buf[0] = 0x82; // Opcode 2
    buf[1] = 0x00;
    buf[2] = (uint8_t)(len & 0xFF);
    buf[3] = (uint8_t)((len >> 8) & 0xFF);
}

uint32_t read_uint32_le(const uint8_t* buf) {
    return (uint32_t)buf[0] | ((uint32_t)buf[1] << 8) | 
           ((uint32_t)buf[2] << 16) | ((uint32_t)buf[3] << 24);
}

// ==========================================
// 2. KCP Client
// ==========================================
class KcpClient {
public:
    KcpClient(boost::asio::io_context& io_context, uint32_t conv)
        : socket_(io_context, udp::endpoint(udp::v4(), 0)),
          running_(true),
          target_set_(false)
    {
        kcp_ = ikcp_create(conv, this);
        kcp_->output = &KcpClient::OutputCallback;

        ikcp_nodelay(kcp_, 1, 10, 2, 1);
        ikcp_wndsize(kcp_, 128, 128);
        ikcp_setmtu(kcp_, 1400);

        StartReceive();

        update_thread_ = std::thread([this]() {
            while (running_) {
                {
                    std::lock_guard<std::mutex> lock(kcp_mutex_);
                    if (kcp_) ikcp_update(kcp_, GetClockMillis());
                }
                std::this_thread::sleep_for(std::chrono::milliseconds(5));
            }
        });
    }

    // 【新增】显式停止方法：打破 ASIO 循环的关键
    void Stop() {
        if (!running_) return;
        running_ = false; // 1. 停止逻辑循环

        // 2. 关闭 Socket：这会强制取消 async_receive_from，触发 operation_aborted
        // 从而让 io_context.run() 可以返回，线程才能 join
        boost::system::error_code ec;
        socket_.close(ec);

        // 3. 等待内部线程结束
        if (update_thread_.joinable()) {
            update_thread_.join();
        }
    }

    ~KcpClient() {
        Stop();
        if (kcp_) {
            ikcp_release(kcp_);
            kcp_ = nullptr;
        }
    }

    uint16_t GetLocalPort() { return socket_.local_endpoint().port(); }

    void SetTarget(const std::string& ip, uint16_t port) {
        std::lock_guard<std::mutex> lock(kcp_mutex_);
        remote_endpoint_ = udp::endpoint(boost::asio::ip::make_address(ip), port);
        target_set_ = true;
    }

    void SendMessage(const APIDown& msg) {
        if (!running_) return;
        std::string payload;
        if (!msg.SerializeToString(&payload)) return;

        std::vector<uint8_t> frame;
        make_header(frame, (uint16_t)payload.size());
        frame.insert(frame.end(), payload.begin(), payload.end());
        
        std::lock_guard<std::mutex> lock(kcp_mutex_);
        if (kcp_) {
            ikcp_send(kcp_, (const char*)frame.data(), frame.size());
            ikcp_flush(kcp_); 
        }
    }

private:
    std::vector<uint8_t> stream_buffer_;

    void OnKcpData(const char* data, size_t len) {
        stream_buffer_.insert(stream_buffer_.end(), data, data + len);

        while (true) {
            size_t buf_len = stream_buffer_.size();
            if (buf_len < 4) break; 

            uint8_t b0 = stream_buffer_[0];
            uint32_t payload_len = 0;
            size_t header_len = 0;
            bool is_proto = false;

            // 探测头
            if (b0 >= 0x80) { // C-Style
                payload_len = (uint16_t)((uint8_t)stream_buffer_[2] | ((uint8_t)stream_buffer_[3] << 8));
                header_len = 4;
                is_proto = true;
            } else if (b0 <= 5) { // Rust-Style
                if (buf_len < 5) break; 
                payload_len = read_uint32_le(stream_buffer_.data() + 1);
                header_len = 5;
                is_proto = true;
            } else {
                stream_buffer_.erase(stream_buffer_.begin());
                continue;
            }

            if (buf_len < header_len + payload_len) break; // Wait more

            if (is_proto) {
                APIUp api_up;
                if (api_up.ParseFromArray(stream_buffer_.data() + header_len, payload_len)) {
                    if (api_up.has_base_status()) {
                        const auto& bs = api_up.base_status();
                        if (bs.has_estimated_odometry()) {
                            std::cout << "[Info] Odom: " 
                            << bs.estimated_odometry().DebugString() << std::endl;
                        } 

                    }
                }
            }
            stream_buffer_.erase(stream_buffer_.begin(), stream_buffer_.begin() + header_len + payload_len);
        }
    }

    static int OutputCallback(const char* buf, int len, ikcpcb* kcp, void* user) {
        auto* client = static_cast<KcpClient*>(user);
        if (client->target_set_ && client->running_) {
            boost::system::error_code ec;
            client->socket_.send_to(boost::asio::buffer(buf, len), client->remote_endpoint_, 0, ec);
        }
        return 0;
    }

    void StartReceive() {
        socket_.async_receive_from(
            boost::asio::buffer(recv_buffer_), sender_endpoint_,
            [this](boost::system::error_code ec, std::size_t bytes_recvd) {
                // 如果 socket 被 Stop() 关闭，ec 会是 operation_aborted
                if (!ec && bytes_recvd > 0) {
                    std::lock_guard<std::mutex> lock(kcp_mutex_);
                    if (kcp_) {
                        ikcp_input(kcp_, recv_buffer_.data(), bytes_recvd);
                        while (true) {
                            int len = ikcp_peeksize(kcp_);
                            if (len <= 0) break;
                            std::vector<char> buf(len);
                            int ret = ikcp_recv(kcp_, buf.data(), len);
                            if (ret > 0) OnKcpData(buf.data(), ret);
                        }
                    }
                }
                
                // 只有在 flag 为 true 时才继续投递任务
                // 当 Stop() 被调用，running_ 变 false，递归停止，UDP 线程得以退出
                if (running_ && ec != boost::asio::error::operation_aborted) {
                    StartReceive();
                }
            });
    }

    static uint32_t GetClockMillis() {
        auto now = std::chrono::steady_clock::now();
        return std::chrono::duration_cast<std::chrono::milliseconds>(now.time_since_epoch()).count();
    }

    udp::socket socket_;
    udp::endpoint remote_endpoint_;
    udp::endpoint sender_endpoint_;
    bool target_set_;
    std::array<char, 4096> recv_buffer_;
    ikcpcb* kcp_;
    std::mutex kcp_mutex_;
    std::thread update_thread_;
    std::atomic<bool> running_;
};

// ==========================================
// 3. WebSocket 
// ==========================================
typedef websocketpp::client<websocketpp::config::asio_client> ws_client;
std::atomic<uint64_t> g_session_id(0);
std::atomic<int> g_kcp_server_port(0);
std::mutex g_state_mutex;
std::condition_variable g_cv;
std::atomic<bool> g_drain_mode(false);

void on_ws_message(ws_client* c, websocketpp::connection_hdl hdl, ws_client::message_ptr msg) {
    if (g_drain_mode) return;
    if (msg->get_opcode() != websocketpp::frame::opcode::binary) return;

    APIUp api_up;
    if (api_up.ParseFromString(msg->get_payload())) {
        std::lock_guard<std::mutex> lock(g_state_mutex);
        if (g_session_id == 0 && api_up.session_id() != 0) {
            g_session_id = api_up.session_id();
            g_cv.notify_all();
        }
        if (api_up.has_kcp_server_status()) {
            int port = api_up.kcp_server_status().server_port();
            if (port != 0 && g_kcp_server_port == 0) {
                g_kcp_server_port = port;
                g_cv.notify_all();
            }
        }
    }
}

void on_ws_open(ws_client* c, websocketpp::connection_hdl hdl) {
    websocketpp::lib::error_code ec;
    auto con = c->get_con_from_hdl(hdl, ec);
    if(!ec) con->get_raw_socket().set_option(boost::asio::ip::tcp::no_delay(true));
}

void ws_send(ws_client* c, websocketpp::connection_hdl hdl, const APIDown& msg) {
    std::string payload;
    msg.SerializeToString(&payload);
    websocketpp::lib::error_code ec;
    c->send(hdl, payload, websocketpp::frame::opcode::binary, ec);
}

// ==========================================
// 4. Main
// ==========================================
int main(int argc, char* argv[]) {
    std::string target_ip = "127.0.0.1";
    if (argc >= 2) target_ip = argv[1];

    std::cout << "[Sys] Connecting to: " << target_ip << std::endl;

    boost::asio::io_context udp_ctx;
    boost::asio::executor_work_guard<boost::asio::io_context::executor_type> work(udp_ctx.get_executor());
    std::thread udp_thread([&udp_ctx]() { udp_ctx.run(); });

    ws_client c;
    c.clear_access_channels(websocketpp::log::alevel::all);
    c.init_asio();
    c.set_open_handler(bind(&on_ws_open, &c, std::placeholders::_1));
    c.set_message_handler(bind(&on_ws_message, &c, std::placeholders::_1, std::placeholders::_2));

    websocketpp::lib::error_code ec;
    auto con = c.get_connection("ws://" + target_ip + ":8439", ec);
    if(ec) { std::cerr << "WS Connect Failed" << std::endl; return -1; }
    websocketpp::connection_hdl hdl = con->get_handle();
    c.connect(con);
    std::thread ws_thread([&c]() { c.run(); });

    // 1. Session
    {
        std::unique_lock<std::mutex> lock(g_state_mutex);
        g_cv.wait(lock, []{ return g_session_id != 0; });
    }
    std::cout << "[Step 1] Session ID: " << g_session_id << std::endl;

    // 2. KCP Setup
    auto kcp_client = std::make_shared<KcpClient>(udp_ctx, (uint32_t)g_session_id);
    
    // 3. Enable KCP
    {
        APIDown msg;
        auto* kcp = msg.mutable_enable_kcp();
        kcp->set_client_peer_port(kcp_client->GetLocalPort());
        auto* cfg = kcp->mutable_kcp_config();
        cfg->set_window_size_snd_wnd(64);
        cfg->set_window_size_rcv_wnd(64);
        cfg->set_interval_ms(10);
        cfg->set_no_delay(true);
        cfg->set_nc(true);
        cfg->set_resend(2);
        ws_send(&c, hdl, msg);
    }

    // 4. Wait Port
    {
        std::unique_lock<std::mutex> lock(g_state_mutex);
        g_cv.wait(lock, []{ return g_kcp_server_port != 0; });
    }
    std::cout << "[Step 4] KCP Port: " << g_kcp_server_port << std::endl;
    kcp_client->SetTarget(target_ip, (uint16_t)g_kcp_server_port);

    // 6. Activate
    {
        APIDown msg;
        msg.set_placeholder_message(true);
        kcp_client->SendMessage(msg);
        std::cout << "[Step 6] KCP Activated" << std::endl;
    }

    // 7. WS Freq -> 1Hz
    {
        APIDown msg;
        msg.set_set_report_frequency(ReportFrequency::Rf1Hz);
        ws_send(&c, hdl, msg);
    }

    g_drain_mode = true; 

    // 10. KCP Freq -> 250Hz
    {
        APIDown msg;
        msg.set_set_report_frequency(ReportFrequency::Rf250Hz);
        kcp_client->SendMessage(msg);
    }

    // 11. Init
    {
        APIDown msg;
        msg.mutable_base_command()->set_api_control_initialize(true);
        kcp_client->SendMessage(msg);
        std::cout << "[Step 11] Base Initialized" << std::endl;
    }

    std::cout << "[Step 12] Start Moving Loop..." << std::endl;
    auto start_time = std::chrono::steady_clock::now();
    auto last_ws_hb = start_time;

    APIDown move_msg;
    auto* xyz = move_msg.mutable_base_command()->mutable_simple_move_command()->mutable_xyz_speed();
    xyz->set_speed_x(0.1); 

    APIDown ws_hb;
    ws_hb.set_placeholder_message(true);

    // 运行 600秒
    while (std::chrono::steady_clock::now() - start_time < std::chrono::seconds(10)) {
        kcp_client->SendMessage(move_msg);

        // WS Heartbeat (1Hz)
        auto now = std::chrono::steady_clock::now();
        if (std::chrono::duration_cast<std::chrono::milliseconds>(now - last_ws_hb).count() >= 1000) {
            ws_send(&c, hdl, ws_hb);
            last_ws_hb = now;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(4));
    }

    // Deinit
    {
        APIDown msg;
        msg.mutable_base_command()->set_api_control_initialize(false);
        ws_send(&c, hdl, msg);
    }
    std::cout << "[Info] Successfully deinitialized base" << std::endl;

    // --- 优雅退出流程 ---
    
    // 1. 先关闭 KCP 客户端（这将取消 UDP 挂起任务）
    kcp_client->Stop(); 
    
    // 2. 再关闭 WebSocket
    c.close(hdl, websocketpp::close::status::normal, "Done");

    // 3. 停止 UDP 上下文（此时应该没有任务了，因为 Stop() 已经关闭了 socket）
    work.reset(); 
    
    // 4. 等待线程结束
    udp_thread.join();
    ws_thread.join();
    
    std::cout << "[Sys] Application Exit" << std::endl;
    return 0;
}