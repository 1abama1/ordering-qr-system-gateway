# Windows 10/11 x64

Установщик: `target/release/bundle/nsis/DASTOR Gateway_0.1.0_x64-setup.exe`. Standalone EXE: `target/release/dastor-gateway.exe`. Installer per-user (currentUser), русский/английский. Требуется WebView2; NSIS при отсутствии предлагает Microsoft bootstrapper. Подписание Authenticode не настроено. Установка автоматически не выполнялась.

1. Запустить setup, выбрать каталог, завершить мастер.
2. Запустить DASTOR Gateway. Первоначально intake на паузе, автозапуск и auto-connect выключены. Backend offline до успешного hardware.v1 enrollment/auth/heartbeat.
3. Проверить Диагностика → симулятор. Для Arduino сначала выключить simulation и выбрать обнаруженный COM-порт на Устройствах.
4. Close при включённом «оставлять в трее» скрывает окно и оставляет агент. Tray: открыть, состояние backend, pause/resume intake, явный Quit. Pause запрещает новые движения; начатая операция продолжает работать по sensor/timeout policy.
5. Автозапуск при входе в Windows включается в Настройках. Он запускает `--background`, скрывая основное окно; после рестарта intake снова на паузе, simulation выключен, interrupted commands не возобновляются.

Данные: `%LOCALAPPDATA%\com.dastor.hardware-gateway\gateway.db`, WAL/SHM рядом. Не копируйте только один db-файл при работающем приложении. Не удаляйте ledger для «починки» неопределённой команды — потеряется защита от replay. Credential Manager содержит отдельную OS-защищённую запись; uninstall не является отзывом устройства на backend.

Сборка: `npm ci`, затем `npm run desktop:build`. Первая сборка скачивает NSIS из Tauri releases с проверкой hash. Иконка воспроизводится `python scripts/create-icon.py`. IPC tests используют MockRuntime с отдельным common-controls v6 manifest. Реальный exe использует manifest Tauri и не требует прав администратора.

Проверены сборка x64 и запуск на текущем Windows build 26300. Отдельная установка на чистые Windows 10 и Windows 11, reboot/autostart и physical serial остаются в стендовом checklist.
