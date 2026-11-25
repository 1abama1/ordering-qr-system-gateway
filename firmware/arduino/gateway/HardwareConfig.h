#pragma once
#include <Arduino.h>
// Review a real wiring diagram before enabling. NO motor outputs are configured by default.
#define DASTOR_REAL_HARDWARE 0
constexpr uint8_t BUTTON_PINS[6] = {2, 3, 4, 5, 6, 7};
constexpr uint16_t DEBOUNCE_MS = 30;
constexpr uint16_t SENSOR_STABLE_MS = 30;
constexpr uint32_t MOVE_TIMEOUT_MS = 14000;
constexpr uint32_t HOST_TIMEOUT_MS = 3000;
constexpr uint32_t SERIAL_BAUD = 115200;
// A configurable linear six-station ENABLE/DIRECTION adapter. -1 = unconfigured.
// External pull-down on ENABLE and a hardwired NC E-stop power interlock are REQUIRED.
constexpr int8_t MOTOR_ENABLE_PIN = -1;
constexpr int8_t MOTOR_DIRECTION_PIN = -1;
constexpr int8_t ESTOP_FEEDBACK_PIN = -1;
constexpr int8_t POSITION_PINS[6] = {-1, -1, -1, -1, -1, -1};
constexpr uint8_t ESTOP_TRIPPED_LEVEL = HIGH; // NC contact to GND: open/broken wire trips.
constexpr uint8_t POSITION_ACTIVE_LEVEL = LOW;
constexpr uint8_t FORWARD_LEVEL = HIGH;
