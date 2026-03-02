#include <sys/time.h>
#include "mongoose.h"
#include "pb_decode.h"
#include "pb_encode.h"
#include "generated/inc/public_api_up.pb.h"
#include "generated/inc/public_api_down.pb.h"

#define EXPECTED_PROTOCOL_MAJOR_VERSION 1

static bool quit = false;

static bool have_max_pos = false;
static bool have_max_speed = false;
static int64_t g_linear_lift_max_pos = 0;
static uint32_t g_linear_lift_max_speed = 0;

static void ws_handler(struct mg_connection *c, int ev, void *ev_data)
{
    switch (ev)
    {
    case MG_EV_WS_OPEN:
    {
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
            if (rx_msg.which_status == APIUp_linear_lift_status_tag)
            {
                LinearLiftStatus ll = rx_msg.status.linear_lift_status;
                if (ll.calibrated)
                {
                    /* Save max_pos and max_speed so the main task can act on it */
                    g_linear_lift_max_pos = ll.max_pos;
                    g_linear_lift_max_speed = ll.max_speed;
                    have_max_pos = true;
                    have_max_speed = true;
                    double pulse_per_meter = (double)ll.pulse_per_rotation;
                    double current_meter = (double)ll.current_pos / pulse_per_meter;
                    double max_meter = (double)ll.max_pos / pulse_per_meter;
                    double percentage = (double)ll.current_pos / (double)ll.max_pos;
                    printf("[LL] Calibrated: true; Current position: %f m, Max position: %f m, Percentage: %f, Raw Current Pos: %" PRId64 ", Raw Max Pos: %" PRId64 "\n",
                           current_meter, max_meter, percentage, (int64_t)ll.current_pos, (int64_t)ll.max_pos);
                }
                else
                {
                    printf("[LL] Lift is not yet calibrated\n");
                }
            }else{
                printf("Received message is not a linear status message\n");
                c->is_closing = 1;
                quit = true;
            }
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
    double percentage = 0.5;
    if (argc > 2)
    {
        percentage = strtod(argv[2], NULL);
        if (!(percentage >= 0.0 && percentage <= 1.0))
        {
            fprintf(stderr, "percentage must be between 0.0 and 1.0\n");
            return 1;
        }
    }
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

    /* We will build LinearLift messages later when we know max_pos and max_speed */
    uint8_t ll_speed_buffer[1024];
    uint8_t ll_target_buffer[1024];
    size_t ll_speed_buffer_size = 0;
    size_t ll_target_buffer_size = 0;

    struct timespec start_time_val, last_send_time_val;
    int count = 0;
    clock_gettime(CLOCK_MONOTONIC, &start_time_val);
    clock_gettime(CLOCK_MONOTONIC, &last_send_time_val);
    /* Wait until we get max pos and speed from lift status */
    while (!have_max_pos || !have_max_speed)
    {
        mg_mgr_poll(&mgr, 20);
        if (quit)
        {
            mg_mgr_free(&mgr);
            return 1;
        }
    }

    /* Compute move target and set speed */
    int64_t move_target = (int64_t)((double)g_linear_lift_max_pos * percentage);
    uint32_t speed = (uint32_t)((double)g_linear_lift_max_speed * 0.9);

    /* Build LinearLift SetSpeed message */
    APIDown ll_speed_msg = APIDown_init_default;
    ll_speed_msg.which_down = APIDown_linear_lift_command_tag;
    ll_speed_msg.down.linear_lift_command.which_command = LinearLiftCommand_set_speed_tag;
    ll_speed_msg.down.linear_lift_command.command.set_speed = speed;
    pb_ostream_t ll_stream = pb_ostream_from_buffer(ll_speed_buffer, sizeof(ll_speed_buffer));
    bool ll_status = pb_encode(&ll_stream, APIDown_fields, &ll_speed_msg);
    if (!ll_status)
    {
        printf("Failed to encode LinearLift SetSpeed message\n");
        quit = true;
    }
    ll_speed_buffer_size = ll_stream.bytes_written;

    /* Build LinearLift TargetPos message */
    APIDown ll_target_msg = APIDown_init_default;
    ll_target_msg.which_down = APIDown_linear_lift_command_tag;
    ll_target_msg.down.linear_lift_command.which_command = LinearLiftCommand_target_pos_tag;
    ll_target_msg.down.linear_lift_command.command.target_pos = move_target;
    ll_stream = pb_ostream_from_buffer(ll_target_buffer, sizeof(ll_target_buffer));
    ll_status = pb_encode(&ll_stream, APIDown_fields, &ll_target_msg);
    if (!ll_status)
    {
        printf("Failed to encode LinearLift TargetPos message\n");
        quit = true;
    }
    ll_target_buffer_size = ll_stream.bytes_written;

    while (!quit)
    {
        // Send init and move messages for 10s, at 50Hz. Lastly, send deinit message.
        struct timespec now_val;
        clock_gettime(CLOCK_MONOTONIC, &now_val);
        double total_elapsed_time = now_val.tv_sec - start_time_val.tv_sec;
        // On MacOS this tv_usec is wired? Seems need to divide by 1000000.0 to get correct value.
        double elapsed_time_ms = (now_val.tv_nsec - last_send_time_val.tv_nsec) / 1000000.0 + (now_val.tv_sec - last_send_time_val.tv_sec) * 1000.0;
        if (elapsed_time_ms >= 20.0)
        {
            mg_ws_send(c, init_buffer, init_buffer_size, WEBSOCKET_OP_BINARY);
            /* Only send set speed once - we can optionally keep sending it, but send once here */
            if (ll_speed_buffer_size > 0)
            {
                mg_ws_send(c, ll_speed_buffer, ll_speed_buffer_size, WEBSOCKET_OP_BINARY);
                /* Set to zero to avoid sending repeatedly */
                ll_speed_buffer_size = 0;
            }
            mg_ws_send(c, ll_target_buffer, ll_target_buffer_size, WEBSOCKET_OP_BINARY);
            clock_gettime(CLOCK_MONOTONIC, &last_send_time_val);
            printf("Sending message at %ld.%06ld.\n", now_val.tv_sec, now_val.tv_nsec);
            count++;
        }
        // Check if 10 seconds have passed since start time
        if (total_elapsed_time >= 5.0)
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
