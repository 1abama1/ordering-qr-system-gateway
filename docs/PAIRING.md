# Регистрация и секреты

1. Backend должен сначала реализовать hardware.v1 (в существующем Spring Boot проекте его пока нет).
2. Оставьте intake на паузе, выключите simulation, убедитесь, что нет движения. Задайте HTTPS origin в Настройках при отключённом serial.
3. ADMIN создаёт одноразовый pairing code на backend. Введите код в Подключения → Регистрация устройства.
4. Gateway вызывает enroll и сохраняет идентичность и secret одним blob в Windows Credential Manager (`com.dastor.hardware-gateway`, user `device`). Ошибка записи делает регистрацию неуспешной; секрет не сохраняется в SQLite или React.
5. Фоновая задача обменивает device credentials на ограниченный token. Нужны scope hardware.v1, отдельный audience на backend и валидный срок. Обновление за 5 секунд до expiry. 401 на Device API сбрасывает token, 401/403 на auth останавливает попытки до нового pairing.
6. Browser binding создаётся сайтом и проверяется backend. Gateway не выбирает заказ и не обладает staff правами.

Повторный pairing — путь rotation/revocation для v1, с новым ADMIN code и атомарным отзывом старого secret на сервере. Origin credentials закреплён; UI запрещает менять origin зарегистрированного устройства. Для административной миграции к другому origin сначала отозвать устройство на backend, завершить Gateway и вручную удалить только его запись в Credential Manager; локальный ledger не удалять.

Diagnostics содержит только whitelist typed snapshot: connection state, безопасные настройки, device/session identifiers, protocol counters и command result events. Secret, token, pairingCode, leaseToken, request/response body и raw serial frames в snapshot отсутствуют. Diagnostics может содержать device identifiers и адрес backend; перед передачей третьим лицам просмотрите файл.
