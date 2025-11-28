#include <sys/time.h>
#include "mongoose.h"
#include "pb_decode.h"
#include "pb_encode.h"
#include "generated/inc/public_api_up.pb.h"
#include "generated/inc/public_api_down.pb.h"

#define EXPECTED_PROTOCOL_MAJOR_VERSION 1

static bool quit = false;

static void ws_handler(struct mg_connection *c, int ev, void *ev_data)
{
    switch (ev)
    {
    case MG_EV_WS_OPEN:
    {
        // Change report frequency to 50Hz, since we don't really need to hear from base too often, do we?
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
        }
        mg_ws_send(c, obuffer, change_frequency_buffer_size, WEBSOCKET_OP_BINARY);
        break;
    }
    case MG_EV_WS_MSG:
    {
        // Check if is binary message
        struct mg_ws_message *wm = (struct mg_ws_message *)ev_data;
        if (wm->flags & WEBSOCKET_OP_BINARY)
        {
            pb_istream_t stream = pb_istream_from_buffer(wm->data.buf, wm->data.len);
            APIUp rx_msg = APIUp_init_zero;
            bool status = pb_decode(&stream, APIUp_fields, &rx_msg);
            if (!status)
            {
                printf("Failed to decode APIUp message\n");
                c->is_closing = 1;
                quit = true;
            }

            if (rx_msg.protocol_major_version != EXPECTED_PROTOCOL_MAJOR_VERSION)
            {
                printf("Protocol major version is not %d, current version: %d. This might cause compatibility issues. Consider upgrading the base firmware.\n", EXPECTED_PROTOCOL_MAJOR_VERSION, rx_msg.protocol_major_version);
                c->is_closing = 1;
                quit = true;
            }
            if (rx_msg.which_status != APIUp_base_status_tag)
            {
                printf("Received message is not a base status message\n");
                c->is_closing = 1;
                quit = true;
            }
            printf("[%ld]Received base status message; SpdX: %f, SpdY %f, SpdZ %f\n", time(NULL), rx_msg.status.base_status.estimated_odometry.speed_x,
                   rx_msg.status.base_status.estimated_odometry.speed_y, rx_msg.status.base_status.estimated_odometry.speed_z);
        }
        break;
    }
    case MG_EV_ERROR:
    {
        // Error occurred, close connection
        c->is_closing = 1;
        break;
    }
    default:
        break;
    }
}

int main(int argc, char *argv[])
{
    const char *url = argc > 1 ? argv[1] : "ws://localhost:8000/websocket";
    struct mg_mgr mgr;
    mg_mgr_init(&mgr);

    mg_log_set(MG_LL_INFO);

    // Initiate WebSocket connection
    struct mg_connection *c = mg_ws_connect(&mgr, url, ws_handler, NULL, NULL);
    if (c == NULL)
    {
        fprintf(stderr, "Failed to start WS connection to %s\n", url);
        mg_mgr_free(&mgr);
        return 1;
    }

    APIDown init_msg = APIDown_init_default;
    init_msg.which_down = APIDown_base_command_tag;
    init_msg.down.base_command.which_command = BaseCommand_api_control_initialize_tag;
    init_msg.down.base_command.command.api_control_initialize = true;
    uint8_t init_buffer[1024];
    pb_ostream_t stream = pb_ostream_from_buffer(init_buffer, sizeof(init_buffer));
    bool status = pb_encode(&stream, APIDown_fields, &init_msg);
    if (!status)
    {
        printf("Failed to encode APIDown message\n");
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
        printf("Failed to encode APIDown message\n");
        quit = true;
    }
    size_t deinit_buffer_size = stream.bytes_written;

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
        printf("Failed to encode APIDown message\n");
        quit = true;
    }

    struct timeval start_time_val, last_send_time_val;
    int count = 0;
    clock_gettime(CLOCK_MONOTONIC, &start_time_val);
    clock_gettime(CLOCK_MONOTONIC, &last_send_time_val);
    while (!quit)
    {
        // Send init and move messages for 10s, at 50Hz. Lastly, send deinit message.
        struct timeval now_val;
        clock_gettime(CLOCK_MONOTONIC, &now_val);
        double total_elapsed_time = now_val.tv_sec - start_time_val.tv_sec;
        // On MacOS this tv_usec is wired? Seems need to divide by 1000000.0 to get correct value.
        double elapsed_time_ms = (now_val.tv_usec - last_send_time_val.tv_usec) / 1000000.0 + (now_val.tv_sec - last_send_time_val.tv_sec) * 1000.0;
        if (elapsed_time_ms >= 20.0)
        {
            mg_ws_send(c, init_buffer, init_buffer_size, WEBSOCKET_OP_BINARY);
            mg_ws_send(c, move_buffer, move_buffer_size, WEBSOCKET_OP_BINARY);
            clock_gettime(CLOCK_MONOTONIC, &last_send_time_val);
            printf("Sending message at %ld.%06ld.\n", now_val.tv_sec, now_val.tv_usec);
            count++;
        }
        // Check if 10 seconds have passed since start time
        if (total_elapsed_time >= 2.0)
        {
            printf("Sending deinit message\n");
            mg_ws_send(c, deinit_buffer, deinit_buffer_size, WEBSOCKET_OP_BINARY);
            // Wait for message to be sent
            while (c->send.len > 0)
            {
                mg_mgr_poll(&mgr, 1);
            }
            c->is_closing = 1;
            mg_mgr_poll(&mgr, 1);
            break;
        }
        mg_mgr_poll(&mgr, 20);
    }
    printf("Sent %d messages\n", count);

    mg_mgr_free(&mgr);
    return 0;
}
