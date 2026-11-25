# Диагностика

| Симптом | Действие |
|---|---|
| Backend 403/404 | В текущем backend нет Device API; реализовать INTEGRATION_REQUIREMENTS. Не подставлять staff JWT |
| Backend auth 401/403 | Credentials отозваны либо неверны: новый ADMIN pairing code, paused intake, повторный enrollment |
| Credentials unavailable | Проверить Windows Credential Manager текущего пользователя. Plaintext fallback отсутствует |
| COM-порта нет | USB data cable, драйвер платы, Device Manager, обновить список. COM3 не предполагается |
| Port unavailable | Закрыть Arduino IDE Serial Monitor/другую программу. Serial открывается эксклюзивно, Single Instance запрещает второй UI |
| Handshake timeout | 115200/8N1, прошивка 1.0.x, BUTTONS+CONVEYOR; Uno reset после open нормален |
| NOT_CONFIGURED | Ожидаемая fail-closed прошивка. Реальный hardware adapter требует проверенной схемы и explicit opt-in |
| BOOT_UNVERIFIED | Осмотреть механизм, стабильный единственный position sensor, released hardware E-stop, RESET_FAULT, затем осмотр Gateway |
| NEEDS_INSPECTION | Не повторять движение. Проверить механизм и датчики, reconnect → свежий STATUS; при необходимости RESET_FAULT, затем явный осмотр |
| Backend offline при движении | Локальная операция завершается по датчику/таймауту; сеть не является аварийной цепью |
| Кнопки видны, сайт не реагирует | Simulation изолирован. Для реальных BTN нужны enrollment, backend online, активный browser binding и frontend consumer |
| Дубликаты/потери BTN | RELEASE без PRESS, повторные seq и повтор PRESS при удержании отбрасываются. Очереди ограничены; события старше 1500 мс не повторяются |
| Simulation fault не снимается | Остановить тест, отключить/снова включить simulator, дождаться STATUS, подтвердить осмотр. Fault-injection flags живут до новой simulator session |
| Окно закрыто, агент работает | Режим tray; для завершения использовать «Завершить агент» или tray Quit |
| Ошибка диска / supervisor | Intake блокируется; serial закрывается, firmware host watchdog останавливает привод. Восстановить диск, перезапустить и проверить механизм |

В Diagnostics: typed safe snapshot, последние 200 сообщений, counters, command journal, JSON export. Raw serial и HTTP bodies не логируются. История 100 command events переживает перезапуск, SQLite journal ограничен 2000. Outbox результатов повторяется с eventId; кнопки не восстанавливаются с диска.

Разработка в sandbox: DNS и loopback HTTP могут блокироваться. Нужны разрешённые cargo/npm загрузки и test execution вне сетевой песочницы. На этой машине AVR compiler требовал запуск вне sandbox из-за Illegal System DLL Relocation. Проверка TLS/сертификатов из-за этого не отключается.
