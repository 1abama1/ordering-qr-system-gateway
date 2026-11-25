#include <ArduinoJson.h>
#include "HardwareAdapter.h"
#ifdef __AVR__
#include <avr/wdt.h>
#endif

constexpr size_t MAX_FRAME=256;
char input[MAX_FRAME+1];
char output[MAX_FRAME+1];
size_t inputSize=0, outputSize=0, outputOffset=0;
bool discarding=false, handshaken=false, moving=false, movingNotice=false;
uint32_t sequence=0, lastHost=0, moveStarted=0, sensorChanged=0;
uint8_t position=0, rawPosition=0, target=0;
char commandId[37]="";
const __FlashStringHelper *fault=nullptr;
const __FlashStringHelper *pendingFault=nullptr;
bool emergencyLatched=false;
SafeHardwareAdapter hardware;
StaticJsonDocument<256> request;
struct Button { bool raw; bool stable; uint32_t changed; } buttons[6];
const __FlashStringHelper *buttonName(uint8_t i) {
  switch(i) {case 0:return F("UP");case 1:return F("DOWN");case 2:return F("LEFT");case 3:return F("RIGHT");case 4:return F("YES");default:return F("NO");}
}
struct ButtonEvent {uint32_t seq;uint8_t button;bool pressed;};
ButtonEvent inputEvents[16];
uint8_t eventHead=0,eventCount=0;

bool outputFree() { return outputSize==0; }
void queue(JsonDocument &doc) {
  if(!outputFree()) return; // bounded: input events may drop; seq exposes gaps
  size_t size=measureJson(doc);
  if(size>MAX_FRAME) return;
  outputSize=serializeJson(doc,output,sizeof(output));
  output[outputSize++]='\n'; outputOffset=0;
}
void pumpOutput() {
  if(!outputSize) return;
  int available=Serial.availableForWrite();
  size_t remaining=outputSize-outputOffset;
  size_t count=remaining < (size_t)available ? remaining : (size_t)available;
  if(count) outputOffset+=Serial.write((const uint8_t*)output+outputOffset,count);
  if(outputOffset==outputSize) {outputSize=0;outputOffset=0;}
}
void simple(const __FlashStringHelper *type) {
  StaticJsonDocument<128> doc;doc[F("v")]=1;doc[F("t")]=type;queue(doc);
}
void execution(const __FlashStringHelper *type,bool arrived=false) {
  StaticJsonDocument<192> doc;doc[F("v")]=1;doc[F("t")]=type;doc[F("commandId")]=commandId;
  if(arrived) doc[F("station")]=target;
  queue(doc);
}
void trip(const __FlashStringHelper *code) {
  hardware.stop();moving=false;movingNotice=false;fault=code;pendingFault=code;
}
void status() {
  StaticJsonDocument<192> doc;doc[F("v")]=1;doc[F("t")]=F("STATE");
  doc[F("state")]=hardware.emergency()?F("E_STOP"):moving?F("MOVING"):fault?F("FAULT"):F("READY");
  if(position) doc[F("station")]=position;else doc[F("station")]=nullptr;
  doc[F("safe")]=hardware.configured() && !hardware.emergency() && !moving && position>0;
  if(moving) doc[F("commandId")]=commandId;else doc[F("commandId")]=nullptr;
  queue(doc);
}
bool canonicalUuid(const char *id) {
  if(!id || strlen(id)!=36) return false;
  bool nonzero=false;
  for(uint8_t i=0;i<36;i++) {
    if(i==8||i==13||i==18||i==23) {if(id[i]!='-') return false;}
    else {if(!((id[i]>='0'&&id[i]<='9')||(id[i]>='a'&&id[i]<='f')))return false;if(id[i]!='0')nonzero=true;}
  }
  return nonzero;
}
bool validNonce(const char *nonce) {
  if(!nonce || strlen(nonce)!=32)return false;
  for(uint8_t i=0;i<32;i++) if(!((nonce[i]>='0'&&nonce[i]<='9')||(nonce[i]>='a'&&nonce[i]<='f')))return false;
  return true;
}
void processLine() {
  if(deserializeJson(request,input,inputSize)) {trip(F("INVALID_JSON"));return;}
  if(!request.is<JsonObject>() || !request[F("v")].is<int>() || request[F("v")].as<int>()!=1 || !request[F("t")].is<const char*>()) {trip(F("INVALID_FRAME"));return;}
  const char *type=request[F("t")];
  if(strcmp_P(type,PSTR("HELLO"))==0) {
    if(request.size()!=3 || !validNonce(request[F("nonce")])) {trip(F("INVALID_HELLO"));return;}
    if(moving) trip(F("HOST_RECONNECTED"));
    if(!outputFree())return;
    StaticJsonDocument<224> doc;doc[F("v")]=1;doc[F("t")]=F("HELLO_ACK");doc[F("nonce")]=request[F("nonce")].as<const char*>();doc[F("fw")]=F("1.0.0");
    JsonArray capabilities=doc.createNestedArray(F("cap"));capabilities.add(F("BUTTONS"));capabilities.add(F("CONVEYOR"));queue(doc);
    handshaken=true;sequence=0;eventHead=0;eventCount=0;lastHost=millis();pendingFault=fault;return;
  }
  if(!handshaken)return;
  if(strcmp_P(type,PSTR("PING"))==0 && request.size()==2) {lastHost=millis();simple(F("PONG"));return;}
  if(strcmp_P(type,PSTR("STATUS"))==0 && request.size()==2) {status();return;}
  if(strcmp_P(type,PSTR("STOP"))==0 && request.size()==2) {trip(F("STOPPED"));return;}
  if(strcmp_P(type,PSTR("RESET_FAULT"))==0 && request.size()==2) {
    if(hardware.configured()&&!hardware.emergency()&&!moving&&position>0) {fault=nullptr;pendingFault=nullptr;emergencyLatched=false;status();}
    else trip(F("UNSAFE_RESET"));return;
  }
  if(strcmp_P(type,PSTR("MOVE_TO"))!=0 || request.size()!=4 || !canonicalUuid(request[F("commandId")]) || !request[F("station")].is<unsigned int>()) {trip(F("INVALID_COMMAND"));return;}
  const char *id=request[F("commandId")];
  unsigned int requested=request[F("station")];
  if(requested<1 || requested>6) {trip(F("INVALID_STATION"));return;}
  if(strcmp(id,commandId)==0) {execution(F("ACK"));return;} // never energize on replay
  if(moving) {trip(F("CONFLICTING_MOTION"));return;}
  if(fault || !hardware.configured() || hardware.emergency() || position==0) {trip(F("NOT_READY"));return;}
  if(!outputFree())return; // no motion without ability to queue ACK
  strncpy(commandId,id,sizeof(commandId));target=(uint8_t)requested;
  execution(F("ACK"));lastHost=millis();
  if(position==target) {moving=false;movingNotice=false;return;} // arrival emitted below from sensor
  if(!hardware.move(position,target)) {trip(F("DRIVER_NOT_READY"));return;}
  moving=true;movingNotice=true;moveStarted=millis();
}
void setup() {
#ifdef __AVR__
  MCUSR=0;wdt_disable();
#endif
  hardware.begin();hardware.stop();
  for(uint8_t i=0;i<6;i++) {pinMode(BUTTON_PINS[i],INPUT_PULLUP);bool pressed=digitalRead(BUTTON_PINS[i])==LOW;buttons[i]={pressed,pressed,0};}
  Serial.begin(SERIAL_BAUD);
  fault=hardware.configured()?F("BOOT_UNVERIFIED"):F("NOT_CONFIGURED");
#ifdef __AVR__
  wdt_enable(WDTO_1S);
#endif
}
void loop() {
#ifdef __AVR__
  wdt_reset();
#endif
  uint32_t now=millis();
  if(hardware.emergency() && !emergencyLatched) {trip(F("E_STOP"));emergencyLatched=true;}
  bool conflict=false;uint8_t sensed=hardware.sensedPosition(conflict);
  if(conflict) trip(F("SENSOR_CONFLICT"));
  if(sensed!=rawPosition) {rawPosition=sensed;sensorChanged=now;position=0;}
  if((uint32_t)(now-sensorChanged)>=SENSOR_STABLE_MS)position=rawPosition;
  if(moving && (uint32_t)(now-lastHost)>HOST_TIMEOUT_MS)trip(F("HOST_TIMEOUT"));
  if(moving && (uint32_t)(now-moveStarted)>MOVE_TIMEOUT_MS)trip(F("POSITION_TIMEOUT"));
  if(moving && position==target) {hardware.stop();moving=false;movingNotice=false;}
  pumpOutput();
  if(outputFree() && pendingFault) {
    StaticJsonDocument<128> doc;doc[F("v")]=1;doc[F("t")]=F("FAULT");doc[F("code")]=pendingFault;queue(doc);pendingFault=nullptr;
  } else if(outputFree() && movingNotice) {execution(F("MOVING"));movingNotice=false;}
  else if(outputFree() && !moving && !fault && commandId[0] && target>0 && position==target) {execution(F("ARRIVED"),true);target=0;}
  // Bound serial work each iteration so input floods cannot starve safety checks.
  uint8_t budget=32;
  while(budget-- && Serial.available()) {
    char c=(char)Serial.read();
    if(c=='\n') {if(!discarding&&inputSize) {input[inputSize]='\0';processLine();}inputSize=0;discarding=false;}
    else if(!discarding) {if(inputSize>=MAX_FRAME) {discarding=true;inputSize=0;trip(F("FRAME_TOO_LARGE"));}else if(c!='\r')input[inputSize++]=c;}
  }
  for(uint8_t i=0;i<6;i++) {
    bool pressed=digitalRead(BUTTON_PINS[i])==LOW;
    if(pressed!=buttons[i].raw) {buttons[i].raw=pressed;buttons[i].changed=now;}
    if(pressed!=buttons[i].stable && (uint32_t)(now-buttons[i].changed)>=DEBOUNCE_MS) {
      buttons[i].stable=pressed;sequence++;
      if(handshaken && eventCount<16) {inputEvents[(eventHead+eventCount)%16]={sequence,i,pressed};eventCount++;}
    }
  }
  if(handshaken && outputFree() && eventCount) {
    ButtonEvent &event=inputEvents[eventHead];
    StaticJsonDocument<160> doc;doc[F("v")]=1;doc[F("t")]=F("BTN");doc[F("seq")]=event.seq;doc[F("b")]=buttonName(event.button);doc[F("edge")]=event.pressed?F("PRESS"):F("RELEASE");queue(doc);
    eventHead=(eventHead+1)%16;eventCount--;
  }
}
