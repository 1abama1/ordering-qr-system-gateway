# План реализации

1. Домен: ограниченный serial codec, контроллер состояний, дедупликация кнопок, durable command ledger/outbox.
2. Адаптеры: Windows COM discovery/serial, protocol simulator, HTTPS hardware.v1, Credential Manager.
3. Supervisor: один владелец hardware state, отдельный network worker, bounded channels, restart recovery, deadlines.
4. Tauri 2: typed IPC, tray, single instance, autostart, React dashboard.
5. Firmware: fail-closed Uno adapter, debounce, протокол, sensor-driven movement.
6. Проверки: Rust/property/HTTP/simulator tests, React tests, Windows build; документировать реальные ограничения.

Соседние проекты прочитаны локально 2026-11-24. Backend commit: 845f823.
Контракт ниже — предложение Gateway, не утверждение о существующем API.
