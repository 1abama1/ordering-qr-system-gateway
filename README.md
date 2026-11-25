# DASTOR Gateway

Windows desktop hardware agent: Rust + Tauri 2 + React/TypeScript. Отдельные serial controller, protocol simulator, SQLite command ledger/outbox и HTTPS Device API adapter. UI и окно не владеют фоновым lifecycle: heartbeat/serial продолжают работать в трее.

**Статус интеграции:** соседний backend пока не реализует Device API, frontend не принимает аппаратные события. Gateway не изображает успешное подключение. Контракт и точные изменения: [INTEGRATION_REQUIREMENTS](docs/INTEGRATION_REQUIREMENTS.md). Физический привод по умолчанию отключён в firmware до настройки по схеме. Полная аппаратная приёмка ещё необходима.

## Запуск разработчика

Нужны Windows 10/11 x64, Rust stable MSVC, Visual Studio C++ Build Tools + Windows SDK, Node >=22.12, WebView2.

```powershell
npm ci
npm run desktop:dev
```

`npm run dev` запускает только frontend на 127.0.0.1:1420; в браузере показывается отсутствие desktop runtime, тестовые показания не подставляются. Для программной проверки без Arduino откройте Диагностику → Включить симулятор. Кнопки → виртуальная клавиатура; Конвейер → движение к станции с подтверждением. Production intake в simulation заблокирован.

## Проверки и сборка

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
npm test
npm run build
npm run desktop:build
```

Lockfiles включены. Создаётся `target/release/dastor-gateway.exe` и NSIS installer в `target/release/bundle/nsis`. Для офлайн EXE: `npm run desktop:build -- --no-bundle`. Tauri может скачивать NSIS и WebView2 bootstrapper при первой сборке. [Инструкция Windows](docs/WINDOWS.md), [отчёт проверок](docs/VERIFICATION.md).

## Безопасное выполнение

- Приём remote commands по умолчанию на паузе после каждого запуска; Interrupted movements не повторяются.
- commandId сохраняется до ack/write; повтор не запускает мотор. SENT записывается до serial write, неизвестный исход требует осмотра.
- ACK ≠ ARRIVED. Прибытие подтверждается датчиком с совпадающими commandId и station.
- USB loss/reset/timeout при движении: NEEDS_INSPECTION, software STOP, запрет нового движения до свежего безопасного STATUS и подтверждения оператора.
- Backend outage: новые remote движения запрещены; текущая операция завершается по локальным датчикам/таймауту. E-stop физически независим от Windows, USB и сети.
- Results outbox at-least-once, button events только короткий TTL без retry. Exactly-once не обещается.
- Credentials только в Windows Credential Manager. Production только TLS, redirects запрещены; release игнорирует debug loopback HTTP setting.
- Нет local HTTP server, shell plugin, arbitrary serial IPC или credentials в React. Локальный main webview, CSP и минимальные capabilities.

## Структура

```
crates/gateway-core/src/  domain, state machine, codec, ports, adapters, supervisor
crates/gateway-core/tests/ safety, property, HTTP contracts, simulator lifecycle
src-tauri/               IPC, event bridge, single instance, tray, autostart, installer
src/                     семь экранов React, strict TS/Zod, Query + Zustand
firmware/arduino/        fail-closed Uno firmware и configurable hardware adapter
docs/                    contracts, pairing, Windows, troubleshooting, verification
```

Supervisor единолично владеет hardware state/serial, network worker имеет отдельное соединение SQLite WAL. Нет shared mutex вокруг serial/HTTP. Каналы ограничены 128/64/32; кнопки не копятся на диске. UI recent logs ограничены 200, results history 100, SQLite journal 2000, outbox блокирует новые commands при 10000 событиях. CommandId tombstones сохраняются бессрочно для дедупликации: это журнал, а не очередь повторного выполнения. Следите за местом на диске.

Настройки хранятся в `%LOCALAPPDATA%/com.dastor.hardware-gateway/gateway.db`. Simulation не сохраняется между запусками. [Serial protocol](docs/SERIAL_PROTOCOL.md), [Device API](docs/HARDWARE_V1.md), [pairing](docs/PAIRING.md), [firmware](firmware/arduino/README.md), [troubleshooting](docs/TROUBLESHOOTING.md).

Версии сверены по npm registry, Cargo resolver и официальным руководствам: [Tauri 2](https://v2.tauri.app/start/), [tray](https://v2.tauri.app/learn/system-tray/), [autostart](https://v2.tauri.app/plugin/autostart/), [tokio-serial](https://docs.rs/tokio-serial/5.5.0/tokio_serial/), [keyring 3.6 Windows backend](https://docs.rs/keyring/3.6.3/keyring/), [ArduinoJson v6 для 8-bit MCU](https://arduinojson.org/v7/how-to/upgrade-from-v6/). Keyring 3.6 и Reqwest 0.12 выбраны как совместимые проверяемые API; они не заявляются последними major версиями.
