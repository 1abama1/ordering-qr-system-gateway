# Serial Protocol v1

115200 baud по умолчанию, 8N1. UTF-8 JSON Lines, до 256 байт JSON без LF; CRLF принимается. Пустые строки игнорируются. Переполненная строка отбрасывается целиком до LF. Gateway не падает на broken UTF-8, JSON, неизвестных полях/типах и неверных диапазонах. Входной chunk <=512 байт; долговременный RX buffer <=256.

| Направление | JSON |
|---|---|
| Host → controller | `{"v":1,"t":"HELLO","nonce":"32 lowercase hex characters"}` |
| Controller → host | `{"v":1,"t":"HELLO_ACK","nonce":"same nonce","fw":"1.0.0","cap":["BUTTONS","CONVEYOR"]}` |
| Host → controller | `{"v":1,"t":"STATUS"}` |
| Controller → host | `{"v":1,"t":"STATE","state":"READY","station":1,"safe":true,"commandId":null}` |
| Controller → host | `{"v":1,"t":"BTN","seq":14,"b":"UP","edge":"PRESS"}` |
| Host → controller | `{"v":1,"t":"MOVE_TO","commandId":"canonical non-nil UUID","station":4}` |
| Controller → host | `{"v":1,"t":"ACK","commandId":"same UUID"}` |
| Controller → host | `{"v":1,"t":"MOVING","commandId":"same UUID"}` |
| Controller → host | `{"v":1,"t":"ARRIVED","commandId":"same UUID","station":4}` |
| Controller → host | `{"v":1,"t":"FAULT","code":"POSITION_TIMEOUT"}` |
| Host → controller | `{"v":1,"t":"STOP"}` / `{"v":1,"t":"RESET_FAULT"}` |
| Liveness | `{"v":1,"t":"PING"}` → `{"v":1,"t":"PONG"}` |

UUID в реальном JSON — lowercase canonical 36 символов; строка-пример выше описательная. Station 1..6; seq uint32 monotonic в serial-сессии. Кнопки UP/DOWN/LEFT/RIGHT/YES/NO, edges PRESS/RELEASE. Firmware 1.0.x, cap BUTTONS+CONVEYOR. Неизвестная прошивка/nonce закрывают соединение. HOME намеренно отсутствует без реального механизма.

После открытия USB Uno может reset: Gateway ждёт 1800 мс, отправляет новый случайный host nonce и выделяет UUID serial session epoch. Handshake timeout 4 с. Различаются transport-open, handshake, sensor-ready и safe. Локальная session epoch не требует криптографического bootId Arduino. После reconnect никакие кнопки и движения не переигрываются.

ACK timeout 2 с, movement timeout Gateway 15 с, firmware 14 с. Host PING каждую секунду; firmware останавливает мотор при отсутствии PING 3 с. Gateway закрывает silent serial после 3,5 с. Serial write timeout 300 мс. Временные интервалы в motor controller используют монотонный clock/millis с корректным unsigned wrap на AVR.

ACK означает приём, ARRIVED — датчик. Даже STATUS READY не завершает неопределённое активное движение. Несовпадающий ARRIVED, unowned motion, USB loss, timeout и reset оставляют NEEDS_INSPECTION. STOP не создаёт ARRIVED. Очистка inspection требует свежего STATUS (<6 с), известной позиции, safe=true, неподвижного READY/ARRIVED и явного подтверждения оператора. Firmware fault отдельно сбрасывается RESET_FAULT, который не включает мотор.

Сетевая политика: при backend offline новые движения запрещены, уже начатое автономно завершается по датчику или локальному таймауту. Локальный PING продолжает работать, пока исправен Gateway. При падении Gateway/Windows firmware watchdog отключает привод; аппаратный E-stop разрывает привод независимо от software. Это не система функциональной безопасности и не замена аппаратной цепи.

Кнопки debounce 30 мс на AVR, bounded ring 16 событий, в Gateway отсутствует auto-repeat. Неизвестный RELEASE, повтор PRESS и seq <= предыдущего отбрасываются. TTL начинается на приёме Gateway, 1500 мс, backend обязан проверить TTL ещё раз. Session+seq не обеспечивает обнаружение физического возраста USB-буфера, поэтому после потери liveness сессия закрывается. Navigation очередь Gateway <=64, без retry и дискового replay.
