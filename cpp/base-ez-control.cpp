#include <iostream>
#include <thread>
#include <chrono>
#include <string>
#include <memory>

#include <websocketpp/config/asio_no_tls_client.hpp>
#include <websocketpp/client.hpp>

#include "public_api_types.pb.h"
#include "public_api_up.pb.h"
#include "public_api_down.pb.h"


typedef websocketpp::client<websocketpp::config::asio_client> client;
websocketpp::connection_hdl g_hdl;
bool g_connected = false;

// 辅助函数：发送 Protobuf 消息
void send_api_down(client* c, websocketpp::connection_hdl hdl, const APIDown& msg) {
    std::string payload;
    if (msg.SerializeToString(&payload)) {
        websocketpp::lib::error_code ec;
        c->send(hdl, payload, websocketpp::frame::opcode::binary, ec);
        if (ec) {
            std::cerr << "[Error] Send failed: " << ec.message() << std::endl;
        }
    } else {
        std::cerr << "[Error] Protobuf serialization failed!" << std::endl;
    }
}

// 接收消息回调
void on_message(client* c, websocketpp::connection_hdl hdl, client::message_ptr msg) {
    APIUp api_up;
    // 解析接收到的二进制数据
    if (api_up.ParseFromString(msg->get_payload())) {
        if (api_up.has_base_status()) {
            const auto& base_status = api_up.base_status();
            if (base_status.has_estimated_odometry()) {
                std::cout << "[Info] Estimated odometry: " 
                          << base_status.estimated_odometry().DebugString() << std::endl;
            }
        } else {
            // 对应 warn!("Unexpected status type...")
            std::cout << "[Warn] Received unexpected status type." << std::endl;
        }
    } else {
        std::cerr << "[Error] Failed to decode websocket message" << std::endl;
    }
}

void on_open(client* c, websocketpp::connection_hdl hdl) {
    g_hdl = hdl;
    g_connected = true;

    websocketpp::lib::error_code ec;
    auto con = c->get_con_from_hdl(hdl);
if (ec) {
        std::cerr << "[Warn] Failed to get connection pointer" << std::endl;
        return;
    }

    boost::asio::ip::tcp::socket& socket = con->get_raw_socket();

    boost::system::error_code boost_ec; 
    socket.set_option(boost::asio::ip::tcp::no_delay(true), boost_ec);
    
    if(boost_ec) { 
        std::cerr << "[Warn] Failed to set TCP_NODELAY: " << boost_ec.message() << std::endl; 
    } else {
        std::cout << "[Info] TCP_NODELAY set successfully" << std::endl;
    }
    
    std::cout << "[Info] Connected to WebSocket" << std::endl;
}

void on_fail(client* c, websocketpp::connection_hdl hdl) {
    std::cerr << "[Error] Connection failed" << std::endl;
    exit(1);
}

int main(int argc, char* argv[]) {
    // 1. 参数解析 (简单模拟 Clap)
    std::string ip = "127.0.0.1";
    uint16_t port = 8439;

    if (argc >= 2) ip = argv[1];
    if (argc >= 3) port = std::stoi(argv[2]);

    std::string uri = "ws://" + ip + ":" + std::to_string(port);
    std::cout << "[Info] Try connecting to: " << uri << std::endl;

    client c;

    // 禁用过多的日志
    c.clear_access_channels(websocketpp::log::alevel::all);
    c.set_access_channels(websocketpp::log::alevel::app);

    // 初始化 ASIO
    c.init_asio();

    // 注册回调
    c.set_open_handler(websocketpp::lib::bind(&on_open, &c, websocketpp::lib::placeholders::_1));
    c.set_fail_handler(websocketpp::lib::bind(&on_fail, &c, websocketpp::lib::placeholders::_1));
    c.set_message_handler(websocketpp::lib::bind(&on_message, &c, websocketpp::lib::placeholders::_1, websocketpp::lib::placeholders::_2));

    websocketpp::lib::error_code ec;
    client::connection_ptr con = c.get_connection(uri, ec);
    if (ec) {
        std::cerr << "[Error] Connection init error: " << ec.message() << std::endl;
        return 1;
    }

    c.connect(con);
    std::thread ws_thread([&c]() {
        c.run();
    });
    while (!g_connected) {
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }


    // 设置报告频率 50Hz
    {
        APIDown msg;
        msg.set_set_report_frequency(ReportFrequency::Rf50Hz); 
        send_api_down(&c, g_hdl, msg);
    }
    
    std::this_thread::sleep_for(std::chrono::milliseconds(50));

    // (ApiControlInitialize = true)
    {
        APIDown msg;
        auto* base_cmd = msg.mutable_base_command();
        base_cmd->set_api_control_initialize(true);
        send_api_down(&c, g_hdl, msg);
    }

    auto start_time = std::chrono::steady_clock::now();
    
    // 持续 10秒
    while (std::chrono::steady_clock::now() - start_time < std::chrono::seconds(600)) {
        // 对应: tokio::time::sleep(20ms)
        std::this_thread::sleep_for(std::chrono::milliseconds(20));

        APIDown msg;
        auto* base_cmd = msg.mutable_base_command();
        auto* simple_cmd = base_cmd->mutable_simple_move_command();
        auto* xyz_speed = simple_cmd->mutable_xyz_speed();
        
        xyz_speed->set_speed_x(0.0);
        xyz_speed->set_speed_y(0.0);
        xyz_speed->set_speed_z(0.1);

        send_api_down(&c, g_hdl, msg);
    }

    {
        APIDown msg;
        auto* base_cmd = msg.mutable_base_command();
        base_cmd->set_api_control_initialize(false);
        send_api_down(&c, g_hdl, msg);
        std::cout << "[Info] Successfully deinitialized base" << std::endl;
    }


    c.close(g_hdl, websocketpp::close::status::normal, "Demo finished");
    if (ws_thread.joinable()) {
        ws_thread.join();
    }

    return 0;
}