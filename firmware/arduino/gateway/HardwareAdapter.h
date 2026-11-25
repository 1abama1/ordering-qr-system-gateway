#pragma once
#include "HardwareConfig.h"

class HardwareAdapter {
public:
  virtual void begin() = 0;
  virtual bool configured() const = 0;
  virtual bool emergency() const = 0;
  virtual uint8_t sensedPosition(bool &conflict) const = 0;
  virtual bool move(uint8_t current, uint8_t target) = 0;
  virtual void stop() = 0;
  virtual ~HardwareAdapter() {}
};

class SafeHardwareAdapter final : public HardwareAdapter {
public:
  void begin() override {
#if DASTOR_REAL_HARDWARE
    static_assert(MOTOR_ENABLE_PIN >= 0 && MOTOR_DIRECTION_PIN >= 0 && ESTOP_FEEDBACK_PIN >= 0,
                  "Configure motor/E-stop pins from approved wiring diagram");
    static_assert(POSITION_PINS[0]>=0 && POSITION_PINS[1]>=0 && POSITION_PINS[2]>=0 && POSITION_PINS[3]>=0 && POSITION_PINS[4]>=0 && POSITION_PINS[5]>=0,
                  "Configure six physical position sensors");
    digitalWrite(MOTOR_ENABLE_PIN, LOW); // low before OUTPUT prevents boot activation
    pinMode(MOTOR_ENABLE_PIN, OUTPUT);
    pinMode(MOTOR_DIRECTION_PIN, OUTPUT);
    pinMode(ESTOP_FEEDBACK_PIN, INPUT_PULLUP);
    for (uint8_t i=0;i<6;i++) pinMode(POSITION_PINS[i], INPUT_PULLUP);
#endif
  }
  bool configured() const override { return DASTOR_REAL_HARDWARE == 1; }
  bool emergency() const override {
#if DASTOR_REAL_HARDWARE
    return digitalRead(ESTOP_FEEDBACK_PIN)==ESTOP_TRIPPED_LEVEL;
#else
    return false;
#endif
  }
  uint8_t sensedPosition(bool &conflict) const override {
    conflict=false;
    uint8_t found=0;
#if DASTOR_REAL_HARDWARE
    for(uint8_t i=0;i<6;i++) if(digitalRead(POSITION_PINS[i])==POSITION_ACTIVE_LEVEL) {
      if(found) {conflict=true;return 0;} found=i+1;
    }
#endif
    return found;
  }
  bool move(uint8_t current,uint8_t target) override {
#if DASTOR_REAL_HARDWARE
    if(!configured() || emergency() || current<1 || current>6 || target<1 || target>6) return false;
    digitalWrite(MOTOR_DIRECTION_PIN,target>current?FORWARD_LEVEL:!FORWARD_LEVEL);
    digitalWrite(MOTOR_ENABLE_PIN,HIGH);
    return true;
#else
    (void)current;(void)target;return false;
#endif
  }
  void stop() override {
#if DASTOR_REAL_HARDWARE
    digitalWrite(MOTOR_ENABLE_PIN,LOW);
#endif
  }
};
