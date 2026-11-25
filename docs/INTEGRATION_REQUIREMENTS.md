# Изменения в соседних проектах

Проверено 2026-11-24 по локальным checkout. Backend 845f823, docs/API_CONTRACT.md, docs/WEBSOCKET_EVENTS.md, docs/ARCHITECTURE.md и SecurityConfig.java. Frontend: src/features/realtime/Provider.tsx и src/pages/kitchen/KitchenPage.tsx. Соседние репозитории не изменялись.

## Что сейчас отсутствует

- `/api/device/**`, device identity, enrollment, access token issuer/audience, command leasing, hardware telemetry и browser binding отсутствуют. `SecurityConfig.anyRequest().denyAll()` запрещает Device API.
- Существующий JWT: audience ordering-staff, 15 минут, без refresh. Он непригоден для учётной записи агента.
- STOMP допускает только `/topic/menu/updates` и `/topic/kitchen/orders`; исходящие hardware destinations не разрешены. Клиентские SEND запрещены.
- KitchenPage не имеет аппаратной навигации, selected-order state, подтверждения YES/NO или binding lifecycle.

Поэтому production end-to-end интеграция **не заявляется**. В Gateway HTTP ошибки отображаются как отсутствие связи; heartbeat не подменяется health-check запросом.

## Backend: необходимая реализация hardware.v1

1. Добавить отдельный модуль hardware с device identity, hashed secret, scopes, revocation, short-lived access token с отдельным audience. Отдельная security filter chain для Device API; не открывать доступ staff endpoints по device token. Public только enrollment/auth с rate limits.
2. Реализовать точные DTO и HTTP-коды из [HARDWARE_V1.md](HARDWARE_V1.md). Unknown fields запрещены. Enrollment code одноразовый, привязан к ресторану/device, с TTL, лимитом попыток; выдаётся ADMIN через новый staff endpoint, например `POST /api/staff/hardware/pairing-codes`.
3. Durable command table + уникальный commandId, device ownership, lease/fencing, expiration, attempt/version. Подтверждение ack атомарно проверяет fence. После ack lease не отзывается досрочно до expiresAt; иначе нужен firmware-level fencing, которого v1 не обещает.
4. Command events dedup по eventId, terminal states не регрессируют при нарушении порядка доставки. NEEDS_INSPECTION блокирует следующие движения до безопасной сверки. Истечение lease не является доказательством, что мотор не двигался.
5. Browser binding создаётся авторизованной KITCHEN/ADMIN сессией на REST, обновляется краткой lease при живой STOMP subscription и снимается при disconnect/logout/expiry. Никогда не считать успешный HTTP приём кнопки подтверждением UI-действия.
6. На каждом BTN проверять TTL <=1500 ms, активный точный session binding, staff authorization, device ownership и `(deviceId,serialSessionId,seq)`. Отсутствие вкладки: drop, без изменения заказа и без очереди для следующей вкладки.
7. Добавить адресное событие, например `/user/queue/hardware.v1/input`, связанное с одной кухонной сессией; обновить inbound/outbound security, receipt policy и docs/WEBSOCKET_EVENTS.md. Нельзя broadcast YES всем кухням. Телеметрия не должна включать credentials.
8. Отзыв credentials немедленно запрещает Device API. Rotation в этой версии — повторный enrollment с новым ADMIN code при paused intake; server атомарно отзывает старый secret. Не возвращать новые секреты в heartbeat/config.

## Frontend: необходимая реализация

1. Provider подписывается на разрешённый адресный hardware destination только после авторизации и binding. Снимает binding при unmount, logout, expired token и потере subscription. Сначала receipt, затем готовность принимать hardware input.
2. Runtime schema hardware.v1, eventId dedup, короткий TTL, точный sessionId и source. RELEASE обновляет только визуальное состояние; не выполняет действия.
3. KitchenPage хранит выбор заказа локально. UP/DOWN: соседний заказ; LEFT/RIGHT: колонка/действие; YES: явное подтверждение текущего допустимого действия; NO: отмена диалога/выбора. Без выбранного заказа YES не меняет бизнес-состояние. Repeat YES/NO запрещён.
4. REST остаётся источником бизнес-изменений и проверяет права. Нельзя транслировать кнопки в произвольные motor commands или DOM key injection. После потери focus/visibility политика должна явно отключать business actions либо требовать kiosk session.

## Приёмка совместной интеграции

Реальный backend должен пройти те же client contract tests, затем отдельно проверяются: enrollment/revocation, fencing 409, истёкший token, закрытая вкладка, две кухонные вкладки, reconnect без replay, сетевой сбой после ack и после физического ARRIVED. Реальный Arduino и мотор проверяются только в специально разрешённом стендовом режиме.
