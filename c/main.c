#include "generated/inc/public_api_down.pb.h"
#include "generated/inc/public_api_up.pb.h"
#include "mongoose.h"
#include "pb_decode.h"
#include "pb_encode.h"
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#define EXPECTED_PROTOCOL_MAJOR_VERSION 1

typedef struct {
  bool quit;
  uint8_t *obuffer;
  struct timespec start_time_val, last_send_time_val;
  bool version_warning_printed;
} ctx_t;

// create a new APIDown message
static APIDown *new_init_msg(bool init) {
  APIDown *msg = calloc(1, sizeof(APIDown));
  if (!msg)
    return NULL;
  *msg = (APIDown)APIDown_init_default;
  msg->which_down = APIDown_base_command_tag;
  msg->down.base_command.which_command = BaseCommand_api_control_initialize_tag;
  msg->down.base_command.command.api_control_initialize = init;
  return msg;
}

static APIDown *new_move_msg(float x, float y, float z) {
  APIDown *msg = calloc(1, sizeof(APIDown));
  if (!msg)
    return NULL;
  *msg = (APIDown)APIDown_init_default;
  msg->which_down = APIDown_base_command_tag;
  msg->down.base_command.which_command = BaseCommand_simple_move_command_tag;
  msg->down.base_command.command.simple_move_command.which_command =
      SimpleBaseMoveCommand_xyz_speed_tag;
  msg->down.base_command.command.simple_move_command.command.xyz_speed.speed_x = x;
  msg->down.base_command.command.simple_move_command.command.xyz_speed.speed_y = y;
  msg->down.base_command.command.simple_move_command.command.xyz_speed.speed_z = z;
  return msg;
}

static void send_msg(struct mg_connection *c, ctx_t *ctx, APIDown *msg) {
  pb_ostream_t stream = pb_ostream_from_buffer(ctx->obuffer, 2048);
  if (pb_encode(&stream, APIDown_fields, msg))
    mg_ws_send(c, ctx->obuffer, stream.bytes_written, WEBSOCKET_OP_BINARY);
}

static void ws_handler(struct mg_connection *c, int ev, void *ev_data) {
  ctx_t *ctx = c->fn_data;
  if (ev == MG_EV_WS_OPEN) {
    // Set report frequency to 50Hz; Since its a simple demo using simple_move_command, we don't need to hear from base too often.
    // If not changed, it will spam Estimated odometry at 1000Hz, which is too much for a simple demo.
    // This will only work for the current session, different sessions have independent report frequency settings.
    APIDown msg = {APIDown_set_report_frequency_tag,{.set_report_frequency = ReportFrequency_Rf50Hz}};
    send_msg(c, ctx, &msg);
  } else if (ev == MG_EV_WS_MSG && ((struct mg_ws_message *)ev_data)->flags & WEBSOCKET_OP_BINARY) {
    struct mg_ws_message *wm = ev_data;
    pb_istream_t stream = pb_istream_from_buffer((uint8_t *)wm->data.buf, wm->data.len);
    APIUp rx_msg = APIUp_init_zero;

    bool status = pb_decode(&stream, APIUp_fields, &rx_msg);
    if (!status)
    {
        printf("Failed to encode APIDown message\n");
        ctx->quit = true;
        return;
    }

    if (rx_msg.protocol_major_version != EXPECTED_PROTOCOL_MAJOR_VERSION) {
      if (!ctx->version_warning_printed) {
        printf("Protocol major version is not %d, current version: %d. This might cause compatibility issues. Consider upgrading the base firmware.\n",
              EXPECTED_PROTOCOL_MAJOR_VERSION, rx_msg.protocol_major_version);
        ctx->version_warning_printed = true;
      }
      return;
    }
    // If protocol major version does not match, lets just stop printing odometry.
    if (rx_msg.which_status == APIUp_base_status_tag )
      printf("[%ld]Received base status message; SpdX: %f, SpdY %f, SpdZ %f\n",
             time(NULL), rx_msg.status.base_status.estimated_odometry.speed_x,
             rx_msg.status.base_status.estimated_odometry.speed_y,
             rx_msg.status.base_status.estimated_odometry.speed_z);
  } else if (ev == MG_EV_ERROR) {
    ctx->quit = true;
  }
}

int main(int argc, char *argv[]) {
  const char *url = argc > 1 ? argv[1] : "ws://localhost:8439";
  ctx_t ctx = {0};
  // allocate a buffer for the output stream
  ctx.obuffer = malloc(2048);

  clock_gettime(CLOCK_MONOTONIC, &ctx.start_time_val);
  ctx.last_send_time_val = ctx.start_time_val;

  struct mg_mgr mgr;
  mg_mgr_init(&mgr);

  mg_log_set(MG_LL_INFO);

  struct mg_connection *c = mg_ws_connect(&mgr, url, ws_handler, &ctx, NULL);
  if (c == NULL) {
    fprintf(stderr, "Failed to connect\n");
    free(ctx.obuffer);
    mg_mgr_free(&mgr);
    return 1;
  }

  APIDown *init_msg = new_init_msg(true);
  APIDown *deinit_msg = new_init_msg(false);
  // Down, base command, command, simple_move_command, vx = 0.0, vy = 0, w = 0.1
  APIDown *move_msg = new_move_msg(0.0, 0, 1);

  // Before sending move command, we need to set initialize the base first.
  send_msg(c, &ctx, init_msg);

  while (!ctx.quit) {
    struct timespec now_val;
    clock_gettime(CLOCK_MONOTONIC, &now_val);
    // Calculate the elapsed time since the last send.
    double elapsed_time_ms =
        (now_val.tv_nsec - ctx.last_send_time_val.tv_nsec) / 1000000.0 +
        (now_val.tv_sec - ctx.last_send_time_val.tv_sec) * 1000.0;

    if (elapsed_time_ms >= 20.0) {
      send_msg(c, &ctx, move_msg);
      ctx.last_send_time_val = now_val;
    }
    double total_elapsed_time = now_val.tv_sec - ctx.start_time_val.tv_sec;

    // This is essential because if base lost control for a long time, it will enter protected state.
    // So lets tell the base we are finishing our control session.
    if (total_elapsed_time >= 10.0) {
      send_msg(c, &ctx, deinit_msg);
      while (c->send.len > 0)
        mg_mgr_poll(&mgr, 1);
      break;
    }
    mg_mgr_poll(&mgr, 20);
  }

  free(init_msg);
  free(deinit_msg);
  free(move_msg);
  free(ctx.obuffer);
  mg_mgr_free(&mgr);
  return 0;
}
