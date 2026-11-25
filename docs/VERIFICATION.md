# Отчёт проверки — 2026-11-24

## Фактически выполнено

- Rust stable 1.99.1 MSVC, Node 26.13.0, npm 11.20.0, Windows build 26300.
- `cargo test --workspace --offline`: **26 тестов** (1 Tauri IPC, 6 HTTP contract, 1 полный device workflow, 16 safety/property, 2 supervisor).
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: успешно. На sandbox filesystem компилятор иногда пишет, что incremental hardlinks заменены копированием; это не предупреждения исходного кода.
- `cargo fmt --all -- --check`: успешно.
- `npm test`: **4 React/contract теста**.
- `npm run build`: strict TypeScript + production Vite bundle успешно.
- `npm run desktop:build`: release EXE и NSIS x64 installer собраны. Authenticode signing отсутствует.
- Arduino CLI 1.6.0, AVR core 1.8.6, ArduinoJson 6.21.5: Uno compile успешно, **14822 bytes flash / 1224 bytes static RAM**, 824 bytes для стека.
- Release EXE запущен. Проверены dashboard, simulator handshake, реальные для simulator JSON Lines, UP и YES PRESS/RELEASE, ручное подтверждение движения → ACKNOWLEDGED → ARRIVED на станции 4.
- Второй экземпляр EXE завершился с кодом 0, оставив первоначальный PID. Закрытие native окна оставило тот же PID в трее; повторный запуск восстановил прежнее окно, simulation и историю.
- После явного graceful Quit финальный пересобранный EXE запущен новым процессом: simulation выключен, intake на паузе, активного движения нет, позиция не выдумывается. Финальное окно оставлено открытым для просмотра.

Финальные файлы сборки: `dastor-gateway.exe` — 15348736 bytes, SHA256 `834e68022a5c958fc07938b5dd1bc14ec2fa393205fc344b947fd62b9377bcfb`; NSIS `DASTOR Gateway_0.1.0_x64-setup.exe` — 4221037 bytes, SHA256 `5e187674fda1c4ae0a4c932a9f8a037eab58de185143be21317f43132780440c`.

Автоматические проверки выполнялись локально. GitHub Actions workflow добавлен, но ещё не запускался на GitHub. HTTP tests и AVR compiler выполнялись вне sandbox после подтверждённых ограничений DNS/loopback и DLL relocation; TLS не отключался.

## Что именно симулировано

Полный device workflow использует настоящий supervisor, SQLite WAL/outbox и Reqwest, loopback HTTP mock и serial fixture, который говорит protocol v1. Проверены device credential auth, config/browser binding, heartbeat/telemetry, YES PRESS/RELEASE с scope header, command lease ACK, движение, отправка ARRIVED и повторная выдача того же commandId с единственным SENT/ACK. Тестовая fixture подставляется только через Rust port, не доступна из IPC или настроек production.

Отдельный UI simulator всегда помечен simulation; production intake и telemetry для него выключены. Сценарии: дубликаты, invalid JSON, потерянный ACK, задержка ARRIVED, reset/disconnect, sensor failure, fault/E-stop. Unit/supervisor tests проверяют ограниченные очереди, независимые одновременные шесть кнопок, TTL/unbound browser, stale seq/session, USB uncertainty, movement timeout, recovery, запрет replay, неверные protocol/firmware версии, redirects и oversized responses.

## Оставшаяся совместная и аппаратная приёмка

1. Реализовать Device API/security/device persistence/leases/browser binding и targeted STOMP consumer в соседних backend/frontend согласно INTEGRATION_REQUIREMENTS. Без этого production end-to-end доставка на сайт невозможна.
2. Получить схему привода/драйвера/датчиков и E-stop, заполнить и проверить hardware adapter, pin uniqueness/levels, торможение, питание, EMI, false sensor transitions и направление. Реальная моторная часть не тестировалась и не включается по умолчанию.
3. Стенд: USB unplug/Uno reset/host power loss во время движения, датчик stuck/bounce, E-stop physical cutoff, максимальный стек Uno, watchdog+bootloader и восстановление без replay. Автотесты мотор не включают.
4. Проверить enrollment/rotation/revocation с реальным backend и OS Credential Manager, истечение токена, два браузера и закрытую кухонную вкладку. Реальные credentials в этой сессии не создавались.
5. Чистая установка/обновление/удаление на Windows 10/11, WebView2 bootstrapper, reboot/autostart и antivirus/подписание. Установщик создан, его установка на систему не выполнялась; autostart в системе не включался.
6. Ручной hardware test mode и HOME отсутствуют до подтверждения реальной схемы/механизма. Редактор reconnect intervals пока не предоставлен: действует фиксированная bounded backoff policy с jitter. Локализация поставляется на русском; английские технические идентификаторы протокола сохранены.

Проект — работающий программный этап с desktop сборкой и проверенной симулированной интеграцией. Полная Definition of Done с физическим конвейером и реальным сайтом пока не подтверждена.
