# hardware.v1 — предлагаемый Device API

Статус: **реализован клиентом и проверяется mock HTTP tests; ещё не реализован соседним backend**.
JSON UTF-8, UTC RFC3339, UUID в lowercase canonical формате, Content-Type application/json.
Production только HTTPS. Разрешение HTTP возможно только явно в debug-сборке, только 127.0.0.1/localhost/[::1].
Redirect запрещён; URL без userinfo, query и fragment. Секреты только в JSON enrollment/auth и Authorization header.

## Enrollment и authentication

`POST /api/device/enroll` без Bearer:
`{"contract":"hardware.v1","pairingCode":"..."}` →
`{"contract":"hardware.v1","deviceId":"uuid","deviceSecret":"..."}`.
Code одноразовый, TTL <=5 минут, rate limit на сервере. Device identity + secret записываются одним credential blob в Windows Credential Manager, привязанным к origin. SQLite не содержит credentials.

`POST /api/device/auth/token`:
`{"contract":"hardware.v1","deviceId":"uuid","deviceSecret":"..."}` →
`{"contract":"hardware.v1","accessToken":"...","expiresAt":"...","scopes":["hardware.v1"]}`.
401/403 означает revoked credentials: intake блокируется, требуется enrollment. Token refresh до expiry; 401 на Device API инвалидирует token и вызывает повторную auth без автоматического повтора motor command. Rotation: новый одноразовый pairing code, повторный enrollment при остановленном intake и безопасном контроллере; старый secret backend отзывает атомарно. Никакого staff JWT.

Все следующие запросы: Bearer device token, X-Hardware-Contract: hardware.v1.

| Метод / путь | Тело / ответ |
|---|---|
| GET /api/device/config | `{contract, deviceId, stationMapping:[1,2,3,4,5,6], browserSessionId:uuid|null, browserSessionExpiresAt:UTC|null}` |
| POST /api/device/heartbeat | `{contract,deviceId,sessionId,state,paused,simulation:false,serialConnected,arduinoDetected,firmwareVersion,protocolVersion,safe,currentPosition,activeCommandId,buttonControllerOnline,faultCode,capabilities,supportedActions}` → 204 |
| GET /api/device/commands/next?waitSeconds=20 | 204 либо Command |
| POST /api/device/commands/{id}/ack | `{contract,leaseToken,version}` → 204 (409 = fence rejected) |
| POST /api/device/commands/{id}/events | `{contract,eventId,commandId,version,kind,occurredAt}` → 204 |
| POST /api/device/buttons/events | `{contract,eventId,deviceId,sessionId,seq,button,edge,occurredAt,expiresAt,browserSessionId,simulation:false}` → 204 |
| POST /api/device/telemetry | `{contract,deviceId,state,simulation:false}` → 204 |

Command: `{contract:"hardware.v1",commandId:uuid,deviceId:uuid,action:"MOVE_TO",targetStation:1..6,orderId:positive integer,issuedAt:UTC,expiresAt:UTC,leaseToken:opaque string,attempt:positive integer,version:1}`.
`targetStation` — физическая станция; mapping сопоставляет шесть столиков станциям, permutation 1..6. Backend выбирает targetStation согласно mapping. Lease действует до expiresAt, backend не отзывает lease до этого срока после успешного ack. Иная семантика требует дополнительного fencing handshake с firmware и нового контракта.

Backend обязан проверять ownership deviceId, lease, version, expiry и сериализовать intake по устройству. `kind`: RECEIVED, SENT, ACKNOWLEDGED, ARRIVED, FAULT, NEEDS_INSPECTION, REJECTED. EventId идемпотентен; outbox доставляется at-least-once, возможен иной порядок — terminal state не регрессирует. Gateway не хранит lease в журнале, ACK-ошибку не повторяет как движение.

## Кнопки / browser session

Config обновляется heartbeat-циклом каждые 5 секунд. Gateway передаёт serial observations после handshake. Поле safe требует свежего STATUS менее 6 секунд и отсутствия блокировки inspection. Gateway объявляет supportedActions с MOVE_TO. Backend проверяет живую авторизованную подписку кухни, binding и срок до 1500 мс для каждой кнопки и дедуплицирует `(deviceId,sessionId,seq)`. При отсутствии браузера возвращается 204 без UI-действия. Frontend повторно проверяет TTL, session и eventId. RELEASE останавливает повтор UP/DOWN. YES и NO выполняются только на PRESS. Кнопки с истекшим сроком и кнопки без привязки отбрасываются.

## Ошибки

400 invalid contract, 401 token expired/revoked, 403 scope/device forbidden, 404 API not installed, 409 fence/session conflict, 429 rate limit, 503 unavailable. Client не логирует body ошибок, credentials или URL с секретами. Нет fallback к staff API. Ответ ограничен 64 KiB.
