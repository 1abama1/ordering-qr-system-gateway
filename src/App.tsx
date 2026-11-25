import { useEffect, useRef, useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
  Activity,
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Cable,
  Check,
  ChevronRight,
  CircleHelp,
  Cpu,
  Download,
  ExternalLink,
  Gauge,
  Keyboard,
  LayoutDashboard,
  Link2,
  List,
  Pause,
  Play,
  Power,
  RefreshCw,
  Settings2,
  ShieldCheck,
  Square,
  TriangleAlert,
  Usb,
  X,
} from "lucide-react";
import {
  desktop,
  execute,
  exportDiagnostics,
  getAutostart,
  getPorts,
  getStatus,
  setAutostart,
  statusSchema,
  type ButtonName,
  type Settings,
  type Status,
} from "./api";
import { pages, useUi } from "./store";
const icons = [LayoutDashboard, Usb, Gauge, Keyboard, Link2, List, Settings2];
const labels: Record<string, string> = {
  DISCONNECTED: "Отключён",
  INITIALIZING: "Инициализация",
  READY: "Готов к работе",
  MOVING: "В движении",
  ARRIVED: "Прибытие подтверждено",
  FAULT: "Ошибка оборудования",
  E_STOP: "Аварийная остановка",
  NEEDS_INSPECTION: "Необходим осмотр",
};
const time = (value: string | null | undefined) =>
  value ? new Date(value).toLocaleTimeString("ru-RU") : "—";
const short = (value: string | null | undefined) =>
  value ? `${value.slice(0, 8)}…` : "—";
type Act = (action: Record<string, unknown>) => void;
function Panel({
  title,
  tag,
  children,
  className = "",
}: {
  title: string;
  tag?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={`panel ${className}`}>
      <div className="panel-head">
        <h2>{title}</h2>
        {tag}
      </div>
      {children}
    </section>
  );
}
function Badge({ ok, children }: { ok: boolean; children: ReactNode }) {
  return (
    <span className={`badge ${ok ? "good" : "muted-badge"}`}>
      <i />
      {children}
    </span>
  );
}
function Empty({ children }: { children: ReactNode }) {
  return <div className="empty">{children}</div>;
}
function Confirm({
  title,
  children,
  onConfirm,
  onClose,
}: {
  title: string;
  children: ReactNode;
  onConfirm: () => void;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    ref.current?.showModal();
  }, []);
  return (
    <dialog ref={ref} onCancel={onClose} aria-labelledby="confirm-title">
      <h2 id="confirm-title">{title}</h2>
      <p>{children}</p>
      <div className="actions">
        <button onClick={onClose}>Отмена</button>
        <button
          className="primary"
          onClick={() => {
            onConfirm();
            onClose();
          }}
        >
          Подтверждаю
        </button>
      </div>
    </dialog>
  );
}
export function ButtonPad({ status, act }: { status: Status; act: Act }) {
  const buttons: { name: ButtonName; icon: typeof ArrowUp; label: string }[] = [
    { name: "UP", icon: ArrowUp, label: "Вверх" },
    { name: "LEFT", icon: ArrowLeft, label: "Влево" },
    { name: "YES", icon: Check, label: "Да" },
    { name: "RIGHT", icon: ArrowRight, label: "Вправо" },
    { name: "DOWN", icon: ArrowDown, label: "Вниз" },
    { name: "NO", icon: X, label: "Нет" },
  ];
  return (
    <div className="button-pad">
      {buttons.map(({ name, icon: Icon, label }) => {
        const latest = status.buttons.find((e) => e.button === name);
        const pressed =
          latest?.edge === "PRESS" &&
          Date.now() - Date.parse(latest.occurredAt) < 2000;
        return (
          <button
            key={name}
            className={`hardware-key key-${name} ${pressed ? "pressed" : ""}`}
            aria-label={`${status.simulation ? "Simulation: " : ""}${label}`}
            disabled={!status.simulation || !status.handshakeComplete}
            onClick={() =>
              act({
                type: "simulate",
                control: { type: "button", button: name },
              })
            }
          >
            <Icon size={23} />
            <b>{name}</b>
            <small>{label}</small>
          </button>
        );
      })}
    </div>
  );
}
function Stations({ status }: { status: Status }) {
  return (
    <div className="stations">
      {status.settings.stationMapping.map((station, index) => (
        <div
          key={station}
          className={`station ${status.position === station ? "current" : ""}`}
        >
          <span>СТАНЦИЯ</span>
          <b>{station.toString().padStart(2, "0")}</b>
          <small>Столик {index + 1}</small>
          <i />
        </div>
      ))}
    </div>
  );
}
function History({ status }: { status: Status }) {
  return status.history.length ? (
    <div className="table-wrap">
      <table>
        <thead>
          <tr>
            <th>Команда</th>
            <th>Событие</th>
            <th>Время</th>
          </tr>
        </thead>
        <tbody>
          {status.history.slice(0, 12).map((h) => (
            <tr key={h.eventId}>
              <td className="mono">{short(h.commandId)}</td>
              <td>{h.kind}</td>
              <td>{time(h.occurredAt)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  ) : (
    <Empty>История команд появится после первого выполнения.</Empty>
  );
}
function Overview({ s, act }: { s: Status; act: Act }) {
  const setPage = useUi((x) => x.setPage);
  return (
    <>
      <div className="hero">
        <div>
          <span className="eyebrow">HARDWARE CONTROL CENTER</span>
          <h2>{labels[s.state]}</h2>
          <p>
            {s.simulation
              ? "Работает симулятор. Production telemetry и удалённые команды отключены."
              : s.handshakeComplete
                ? "Контроллер проверен. Готовность определяется физическими датчиками."
                : "Подключите контроллер и завершите проверку оборудования."}
          </p>
        </div>
        <div className={`hero-orbit ${s.safe ? "online" : ""}`}>
          <Cpu size={38} />
          <span />
          <span />
        </div>
      </div>
      <div className="metrics">
        <Metric
          title="Backend"
          value={s.backendConnected ? "Подключён" : "Нет соединения"}
          icon={<Link2 />}
          good={s.backendConnected}
          detail={
            s.lastHeartbeat
              ? `Heartbeat ${time(s.lastHeartbeat)}`
              : "Heartbeat ещё не подтверждён"
          }
        />
        <Metric
          title="Arduino"
          value={
            s.handshakeComplete
              ? "Контроллер найден"
              : s.usbConnected
                ? "USB подключён"
                : "Не подключена"
          }
          icon={<Usb />}
          good={s.handshakeComplete}
          detail={s.firmware ? `Firmware ${s.firmware}` : "Ожидается handshake"}
        />
        <Metric
          title="Конвейер"
          value={labels[s.state]}
          icon={<Gauge />}
          good={s.safe}
          detail={
            s.position
              ? `Физическая станция ${s.position}`
              : "Позиция не определена"
          }
        />
        <Metric
          title="События кнопок"
          value={String(s.statistics.buttonEvents)}
          icon={<Keyboard />}
          good={!!s.browserSessionId}
          detail={
            s.browserSessionId
              ? "Есть привязанная веб-сессия"
              : "Веб-сессия не привязана"
          }
        />
      </div>
      <div className="grid-two">
        <Panel
          title="Физические позиции"
          tag={
            <button className="text-button" onClick={() => setPage("Конвейер")}>
              Открыть <ChevronRight size={14} />
            </button>
          }
        >
          <Stations status={s} />
          <div className="panel-foot">
            <span>
              <i className="legend-dot" /> Подтверждение датчиком
            </span>
            <span>
              {s.activeCommand
                ? `Команда ${short(s.activeCommand)}`
                : "Нет активного движения"}
            </span>
          </div>
        </Panel>
        <Panel title="Цепочка подключения">
          <div className="connection-chain">
            {[
              ["Gateway зарегистрирован", s.enrolled],
              ["Backend авторизован", s.backendConnected],
              ["Serial handshake", s.handshakeComplete],
              ["Безопасное состояние", s.safe],
            ].map(([label, ok]) => (
              <div key={String(label)}>
                <span className={`chain-icon ${ok ? "ok" : ""}`}>
                  <Check size={14} />
                </span>
                <span>{label}</span>
                <small>{ok ? "Проверено" : "Ожидается"}</small>
              </div>
            ))}
          </div>
        </Panel>
      </div>
      <div className="grid-two">
        <Panel
          title="Последние события"
          tag={
            <span className="subtle">
              {s.simulation ? "SIMULATION" : "HARDWARE INPUT"}
            </span>
          }
        >
          <ButtonEvents status={s} />
        </Panel>
        <Panel title="Активная команда">
          <div className="command-card">
            <Activity size={28} />
            <b>
              {s.activeCommand
                ? short(s.activeCommand)
                : "Движение не выполняется"}
            </b>
            <p>
              {s.activeCommand
                ? "Ожидается подтверждение датчика ARRIVED."
                : "ACK не считается подтверждением прибытия."}
            </p>
            {s.activeCommand && (
              <button className="danger" onClick={() => act({ type: "stop" })}>
                <Square size={14} /> STOP
              </button>
            )}
          </div>
        </Panel>
      </div>
    </>
  );
}
function Metric({
  title,
  value,
  icon,
  good,
  detail,
}: {
  title: string;
  value: string;
  icon: ReactNode;
  good: boolean;
  detail: string;
}) {
  return (
    <section className="metric">
      <div>
        <span>{title}</span>
        <span className={`metric-icon ${good ? "ok" : ""}`}>{icon}</span>
      </div>
      <b>{value}</b>
      <small>{detail}</small>
    </section>
  );
}
function ButtonEvents({ status }: { status: Status }) {
  return status.buttons.length ? (
    <div className="event-list">
      {status.buttons.slice(0, 6).map((e) => (
        <div key={e.eventId}>
          <span className="event-key">{e.button}</span>
          <span>
            {e.edge === "PRESS" ? "Нажатие" : "Отпускание"}{" "}
            <small>{e.simulation ? "SIM" : "USB"}</small>
          </span>
          <time>{time(e.occurredAt)}</time>
        </div>
      ))}
    </div>
  ) : (
    <Empty>
      <Keyboard size={25} />
      <span>
        Нажмите физическую кнопку
        <br />
        или включите симулятор.
      </span>
    </Empty>
  );
}
function Devices({ s, act }: { s: Status; act: Act }) {
  const ports = useQuery({
    queryKey: ["ports"],
    queryFn: getPorts,
    refetchInterval: 5000,
  });
  const [port, setPort] = useState(s.settings.port ?? "");
  return (
    <div className="grid-two">
      <Panel
        title="USB Serial"
        tag={
          <button onClick={() => void ports.refetch()}>
            <RefreshCw size={14} /> Обновить
          </button>
        }
      >
        <p className="panel-copy">
          COM-порт проходит проверку HELLO → HELLO_ACK → STATUS. Обнаружение USB
          не означает готовность.
        </p>
        {ports.isError && <p role="alert">Не удалось обнаружить порты.</p>}
        <div className="port-list">
          {ports.data?.map((p) => (
            <label
              key={p.name}
              className={`port ${port === p.name ? "selected" : ""}`}
            >
              <input
                type="radio"
                name="port"
                value={p.name}
                checked={port === p.name}
                onChange={() => setPort(p.name)}
              />
              <Usb size={23} />
              <span>
                <b>
                  {p.name} · {p.product ?? "Serial controller"}
                </b>
                <small>
                  {p.manufacturer ?? "Производитель неизвестен"} · VID{" "}
                  {p.vid?.toString(16) ?? "—"} / PID{" "}
                  {p.pid?.toString(16) ?? "—"}
                  <br />
                  Serial {p.serialNumber ?? "—"}
                </small>
              </span>
            </label>
          ))}
        </div>
        {!ports.data?.length && (
          <Empty>COM-порты не найдены. Подключите Arduino по USB.</Empty>
        )}
        <div className="actions">
          <button
            className="primary"
            disabled={!port || s.usbConnected || s.simulation}
            onClick={() => act({ type: "connect", port })}
          >
            <Cable size={15} /> Подключить
          </button>
          <button
            disabled={!s.usbConnected}
            onClick={() => act({ type: "disconnect" })}
          >
            Отключить
          </button>
        </div>
      </Panel>
      <Panel title="Контроллер">
        <dl>
          <dt>USB</dt>
          <dd>{s.usbConnected ? "Открыт" : "Отключён"}</dd>
          <dt>Handshake</dt>
          <dd>{s.handshakeComplete ? "Завершён" : "Не завершён"}</dd>
          <dt>Прошивка</dt>
          <dd>{s.firmware ?? "—"}</dd>
          <dt>Возможности</dt>
          <dd>{s.capabilities.join(" · ") || "—"}</dd>
          <dt>Serial session</dt>
          <dd className="mono">{short(s.sessionId)}</dd>
          <dt>Источник</dt>
          <dd>{s.simulation ? "Симулятор" : "USB Serial"}</dd>
        </dl>
        <div className="info-note">
          <ShieldCheck size={18} /> После reconnect старые движения не
          запускаются автоматически.
        </div>
      </Panel>
    </div>
  );
}
function Conveyor({ s, act }: { s: Status; act: Act }) {
  const [confirm, setConfirm] = useState<number | "inspect" | "reset" | null>(
    null,
  );
  return (
    <>
      <Panel
        title="Конвейер · шесть физических позиций"
        tag={<Badge ok={s.safe}>{labels[s.state]}</Badge>}
      >
        <Stations status={s} />
        <div className="actions">
          <button
            className="danger"
            disabled={!s.usbConnected}
            onClick={() => act({ type: "stop" })}
          >
            <Square size={15} /> STOP
          </button>
          <button
            disabled={!s.handshakeComplete}
            onClick={() => setConfirm("inspect")}
          >
            <ShieldCheck size={15} /> Подтвердить осмотр
          </button>
          <button
            disabled={
              !s.handshakeComplete || s.position === null || !!s.activeCommand
            }
            onClick={() => setConfirm("reset")}
          >
            <RefreshCw size={15} /> Сбросить fault
          </button>
          <span className="subtle">
            Software STOP не заменяет аппаратный E-stop.
          </span>
        </div>
        {s.fault && (
          <div className="warning">
            <TriangleAlert size={18} />
            <span>{s.fault} · требуется проверка оборудования</span>
          </div>
        )}
      </Panel>
      <div className="grid-two">
        <Panel title="Тест движения · Simulation">
          <p className="panel-copy">
            Ручные движения доступны в симуляторе после проверки готовности.
            Реальное движение принимается только через Device API и безопасный
            hardware adapter.
          </p>
          <div className="actions">
            {[1, 2, 3, 4, 5, 6].map((station) => (
              <button
                key={station}
                disabled={!s.simulation || !s.safe || !!s.activeCommand}
                onClick={() => setConfirm(station)}
              >
                Станция {station}
              </button>
            ))}
          </div>
        </Panel>
        <Panel title="Текущее выполнение">
          <dl>
            <dt>Состояние</dt>
            <dd>{labels[s.state]}</dd>
            <dt>Позиция</dt>
            <dd>{s.position ?? "Неизвестна"}</dd>
            <dt>Команда</dt>
            <dd className="mono">{short(s.activeCommand)}</dd>
            <dt>Безопасность</dt>
            <dd>{s.safe ? "Проверена" : "Не подтверждена"}</dd>
          </dl>
        </Panel>
      </div>
      <Panel title="Журнал выполнения">
        <History status={s} />
      </Panel>
      {confirm !== null && (
        <Confirm
          title={
            confirm === "inspect"
              ? "Подтверждение осмотра"
              : confirm === "reset"
                ? "Сброс аппаратной ошибки"
                : "Тестовое движение"
          }
          onClose={() => setConfirm(null)}
          onConfirm={() =>
            act(
              confirm === "inspect"
                ? { type: "inspect", confirmed: true }
                : confirm === "reset"
                  ? { type: "reset_fault", confirmed: true }
                  : { type: "manual_move", station: confirm, confirmed: true },
            )
          }
        >
          {confirm === "inspect"
            ? "Подтверждаю осмотр механизма, отсутствие людей в зоне движения и безопасный неподвижный STATUS от контроллера."
            : confirm === "reset"
              ? "Подтверждаю, что механизм неподвижен, датчик позиции исправен, аппаратный E-stop освобождён. RESET_FAULT не включает мотор. После сброса подтвердите осмотр отдельно."
              : `Запустить симулированное движение к станции ${confirm}? Это не сигнал реальному мотору.`}
        </Confirm>
      )}
    </>
  );
}
function Buttons({ s, act }: { s: Status; act: Act }) {
  return (
    <>
      <div className="grid-two">
        <Panel
          title="Шестикнопочная клавиатура"
          tag={
            <Badge ok={!s.simulation}>
              {s.simulation ? "SIMULATION" : "HARDWARE INPUT"}
            </Badge>
          }
        >
          <ButtonPad status={s} act={act} />
          <p className="panel-copy centered">
            {s.simulation
              ? "Виртуальные кнопки отправляют события через protocol simulator."
              : "Подсветка появляется только при реальном PRESS от контроллера."}
          </p>
        </Panel>
        <Panel title="Доставка событий">
          <dl>
            <dt>Веб-сессия кухни</dt>
            <dd className="mono">{short(s.browserSessionId)}</dd>
            <dt>Принято</dt>
            <dd>{s.statistics.buttonEvents}</dd>
            <dt>Дубликаты / некорректные переходы</dt>
            <dd>{s.statistics.duplicates}</dd>
            <dt>Отброшено / недоставлено</dt>
            <dd>{s.statistics.dropped}</dd>
            <dt>Последнее событие</dt>
            <dd>{time(s.buttons[0]?.occurredAt)}</dd>
            <dt>TTL</dt>
            <dd>1 500 мс · без replay</dd>
          </dl>
          <div className="info-note">
            <CircleHelp size={18} /> Без активной авторизованной веб-сессии
            кнопки не вызывают бизнес-действий.
          </div>
        </Panel>
      </div>
      <Panel title="Входящие события">
        <ButtonEvents status={s} />
      </Panel>
    </>
  );
}
function Connections({ s, act }: { s: Status; act: Act }) {
  const [code, setCode] = useState("");
  return (
    <div className="grid-two">
      <Panel
        title="Backend · hardware.v1"
        tag={
          <Badge ok={s.backendConnected}>
            {s.backendConnected ? "Online" : "Offline"}
          </Badge>
        }
      >
        <dl>
          <dt>Origin</dt>
          <dd className="mono break">{s.settings.backendUrl}</dd>
          <dt>TLS</dt>
          <dd>
            {s.settings.backendUrl.startsWith("https:")
              ? "HTTPS обязателен"
              : "Локальный debug HTTP"}
          </dd>
          <dt>Device ID</dt>
          <dd className="mono">{short(s.deviceId)}</dd>
          <dt>Credentials</dt>
          <dd>
            {s.enrolled
              ? "Сохранены в OS Credential Manager"
              : "Не зарегистрированы"}
          </dd>
          <dt>Heartbeat</dt>
          <dd>{time(s.lastHeartbeat)}</dd>
          <dt>Session</dt>
          <dd className="mono">{short(s.sessionId)}</dd>
        </dl>
        <div className="actions">
          <button onClick={() => act({ type: "reconnect_backend" })}>
            <RefreshCw size={15} /> Переподключить
          </button>
        </div>
        <div className="warning">
          <TriangleAlert size={17} />
          <span>
            Device API пока отсутствует в соседнем backend. Нужные изменения
            описаны в INTEGRATION_REQUIREMENTS.md.
          </span>
        </div>
      </Panel>
      <Panel title="Регистрация устройства">
        <p className="panel-copy">
          Введите временный pairing code, созданный администратором ресторана.
          Для повторной регистрации остановите intake. Staff JWT не
          используется.
        </p>
        <label className="field">
          Pairing code
          <input
            value={code}
            onChange={(e) => setCode(e.target.value)}
            autoComplete="off"
            type="password"
            maxLength={64}
            placeholder="Одноразовый код"
          />
        </label>
        <div className="actions">
          <button
            className="primary"
            disabled={
              code.length < 6 || !s.paused || s.simulation || !!s.activeCommand
            }
            onClick={() => {
              act({ type: "enroll", code });
              setCode("");
            }}
          >
            <Link2 size={15} /> Зарегистрировать
          </button>
        </div>
        <div className="info-note">
          <ShieldCheck size={18} /> Device secret и access token никогда не
          передаются в интерфейс.
        </div>
      </Panel>
    </div>
  );
}
const scenarios = [
  ["duplicate", "Дубликат следующего нажатия"],
  ["lost_ack", "Потерянный ACK"],
  ["invalid_json", "Некорректный JSON"],
  ["delayed", "Задержка прибытия"],
  ["reset", "Reset Arduino"],
  ["disconnect", "Потеря USB"],
  ["sensor_failure", "Отказ датчика"],
  ["fault", "Hardware fault"],
  ["e_stop", "E-stop"],
] as const;
function Diagnostics({
  s,
  act,
  onExport,
}: {
  s: Status;
  act: Act;
  onExport: () => void;
}) {
  const [filter, setFilter] = useState("all");
  return (
    <>
      <div className="grid-two">
        <Panel
          title="Protocol statistics"
          tag={<span className="subtle">SERIAL V1</span>}
        >
          <dl>
            <dt>Frames</dt>
            <dd>{s.statistics.frames}</dd>
            <dt>Некорректные frames</dt>
            <dd>{s.statistics.invalid}</dd>
            <dt>Отключения</dt>
            <dd>{s.statistics.reconnects}</dd>
            <dt>Дубликаты кнопок</dt>
            <dd>{s.statistics.duplicates}</dd>
          </dl>
          <div className="actions">
            <button onClick={onExport}>
              <Download size={15} /> Экспорт JSON
            </button>
          </div>
        </Panel>
        <Panel
          title="Hardware simulation"
          tag={
            <Badge ok={s.simulation}>
              {s.simulation ? "Включена" : "Выключена"}
            </Badge>
          }
        >
          <p className="panel-copy">
            Симуляция изолирована от production telemetry. Сценарии ACK и
            датчика применяются к следующему движению.
          </p>
          <div className="actions">
            <button
              className="primary"
              disabled={!!s.activeCommand}
              onClick={() =>
                act({ type: "simulation", enabled: !s.simulation })
              }
            >
              {s.simulation ? "Выключить" : "Включить"} симулятор
            </button>
          </div>
          <div className="scenario-list">
            {scenarios.map(([kind, label]) => (
              <button
                disabled={!s.simulation || !s.handshakeComplete}
                key={kind}
                onClick={() =>
                  act({ type: "simulate", control: { type: "scenario", kind } })
                }
              >
                {label}
              </button>
            ))}
          </div>
        </Panel>
      </div>
      <Panel
        title="Журнал подключений и ошибок"
        tag={
          <select
            aria-label="Фильтр журнала"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          >
            <option value="all">Все уровни</option>
            <option value="error">Ошибки</option>
            <option value="warn">Предупреждения</option>
            <option value="info">Информация</option>
          </select>
        }
      >
        {s.logs.filter((l) => filter === "all" || l.level === filter).length ? (
          <div className="logs">
            {s.logs
              .filter((l) => filter === "all" || l.level === filter)
              .map((l, index) => (
                <div key={`${l.at}-${index}`}>
                  <time>{time(l.at)}</time>
                  <span className={`log-level ${l.level}`}>
                    {l.level.toUpperCase()}
                  </span>
                  <span>{l.message}</span>
                </div>
              ))}
          </div>
        ) : (
          <Empty>Нет записей выбранного уровня.</Empty>
        )}
      </Panel>
    </>
  );
}
function SettingsPage({
  s,
  act,
  onError,
}: {
  s: Status;
  act: Act;
  onError: (message: string) => void;
}) {
  const [settings, setSettings] = useState<Settings>(s.settings);
  const auto = useQuery({ queryKey: ["autostart"], queryFn: getAutostart });
  const qc = useQueryClient();
  const patch = <K extends keyof Settings>(key: K, value: Settings[K]) =>
    setSettings((current) => ({ ...current, [key]: value }));
  return (
    <div className="grid-two">
      <Panel title="Настройки подключения">
        <p className="panel-copy">
          Для сохранения отключите serial. Origin зарегистрированного устройства
          закреплён за credentials.
        </p>
        <label className="field">
          Backend URL
          <input
            value={settings.backendUrl}
            onChange={(e) => patch("backendUrl", e.target.value)}
          />
        </label>
        <label className="field">
          Serial baud
          <select
            value={settings.baud}
            onChange={(e) => patch("baud", Number(e.target.value))}
          >
            {[9600, 19200, 38400, 57600, 115200, 230400].map((v) => (
              <option key={v}>{v}</option>
            ))}
          </select>
        </label>
        <label className="check-field">
          <input
            type="checkbox"
            checked={settings.autoConnect}
            onChange={(e) => patch("autoConnect", e.target.checked)}
          />
          Автоподключение сохранённого COM-порта
        </label>
        <label className="field">
          Столики 1–6 → физические станции
          <div className="mapping">
            {settings.stationMapping.map((v, i) => (
              <input
                key={i}
                aria-label={`Станция для столика ${i + 1}`}
                type="number"
                min={1}
                max={6}
                value={v}
                onChange={(e) =>
                  patch(
                    "stationMapping",
                    settings.stationMapping.map((x, j) =>
                      j === i ? Number(e.target.value) : x,
                    ),
                  )
                }
              />
            ))}
          </div>
        </label>
        <p className="subtle">
          Reconnect: exponential backoff 0,25–30 с с jitter. Настройка
          фиксирована безопасной политикой.
        </p>
        <div className="actions">
          <button
            className="primary"
            disabled={s.usbConnected}
            onClick={() => act({ type: "save_settings", settings })}
          >
            Сохранить настройки
          </button>
        </div>
      </Panel>
      <Panel title="Приложение">
        <label className="check-field">
          <input
            type="checkbox"
            disabled={auto.isPending || auto.isError}
            checked={auto.data ?? false}
            onChange={(e) => {
              void setAutostart(e.target.checked)
                .then(() => qc.invalidateQueries({ queryKey: ["autostart"] }))
                .catch((e) => onError(String(e)));
            }}
          />
          Запускать при входе в Windows
        </label>
        {auto.isError && (
          <p role="alert">Не удалось прочитать состояние автозапуска.</p>
        )}
        <label className="check-field">
          <input
            type="checkbox"
            checked={settings.minimizeToTray}
            onChange={(e) => patch("minimizeToTray", e.target.checked)}
          />
          При закрытии оставлять агент в трее
        </label>
        <label className="field">
          Тема
          <select
            value={settings.theme}
            onChange={(e) =>
              patch("theme", e.target.value as Settings["theme"])
            }
          >
            <option value="dark">Тёмная</option>
            <option value="light">Светлая</option>
          </select>
        </label>
        <label className="field">
          Язык
          <select value="ru" disabled>
            <option value="ru">Русский</option>
          </select>
        </label>
        <div className="info-note">
          <Activity size={18} /> Heartbeat работает при скрытом окне. После
          перезапуска движения не возобновляются.
        </div>
      </Panel>
    </div>
  );
}
export default function App() {
  const { page, setPage } = useUi();
  const qc = useQueryClient();
  const query = useQuery({
    queryKey: ["status"],
    queryFn: getStatus,
    refetchInterval: desktop ? 1000 : false,
    retry: false,
  });
  const [error, setError] = useState("");
  const [quit, setQuit] = useState(false);
  const mutation = useMutation({
    mutationFn: execute,
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["status"] });
    },
    onError: (e) => setError(String(e)),
  });
  const act: Act = (action) => mutation.mutate(action);
  const s = query.data;
  useEffect(() => {
    if (!desktop) return;
    const unlisten = listen("gateway:status", (event) => {
      const result = statusSchema.safeParse(event.payload);
      if (result.success) qc.setQueryData(["status"], result.data);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [qc]);
  useEffect(() => {
    document.documentElement.dataset.theme = s?.settings.theme ?? "dark";
  }, [s?.settings.theme]);
  const onExport = () => {
    void exportDiagnostics()
      .then((text) => {
        const url = URL.createObjectURL(
          new Blob([text], { type: "application/json" }),
        );
        const a = document.createElement("a");
        a.href = url;
        a.download = `dastor-diagnostics-${new Date().toISOString().slice(0, 10)}.json`;
        a.click();
        setTimeout(() => URL.revokeObjectURL(url), 1000);
      })
      .catch((e) => setError(String(e)));
  };
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-symbol">
            D<span />
          </span>
          <div>
            <b>DASTOR</b>
            <span>HARDWARE GATEWAY</span>
          </div>
        </div>
        <span className="nav-label">РАБОЧЕЕ ПРОСТРАНСТВО</span>
        <nav aria-label="Основная навигация">
          {pages.map((name, i) => {
            const Icon = icons[i];
            return (
              <button
                key={name}
                className={page === name ? "active" : ""}
                onClick={() => setPage(name)}
              >
                <Icon size={18} />
                {name}
                {page === name && <i />}
              </button>
            );
          })}
        </nav>
        <div className="sidebar-bottom">
          <div className="agent-card">
            <span
              className={`agent-dot ${s?.backendConnected ? "live" : ""}`}
            />
            <div>
              <b>{s?.simulation ? "Simulation agent" : "Gateway agent"}</b>
              <small>
                {s?.enrolled ? short(s.deviceId) : "Не зарегистрирован"}
              </small>
            </div>
            <span>v0.1</span>
          </div>
          <button
            className="quit"
            disabled={!desktop}
            onClick={() => setQuit(true)}
          >
            <Power size={15} /> Завершить агент
          </button>
        </div>
      </aside>
      <main>
        <header className="topbar">
          <div>
            <span>Workspace</span>
            <ChevronRight size={13} />
            <b>{page}</b>
          </div>
          <div>
            <Badge ok={s?.backendConnected ?? false}>
              {s?.simulation
                ? "Simulation"
                : s?.backendConnected
                  ? "Backend connected"
                  : "Backend offline"}
            </Badge>
            <span className="topbar-divider" />
            <ShieldCheck size={17} />
            <span className="subtle">hardware.v1</span>
          </div>
        </header>
        <div className="content">
          <div className="page-heading">
            <div>
              <span className="eyebrow">DASTOR / GATEWAY</span>
              <h1>{page === "Обзор" ? "Центр управления" : page}</h1>
              <p>
                {page === "Обзор"
                  ? "Оборудование, подключения и события — в одном месте."
                  : "Контроль оборудования ресторана"}
              </p>
            </div>
            <div className="actions">
              {s && (
                <>
                  <span className="intake-state">
                    Intake {s.paused ? "приостановлен" : "активен"}
                  </span>
                  <button
                    disabled={s.simulation}
                    onClick={() => act({ type: "pause", paused: !s.paused })}
                  >
                    {s.paused ? <Play size={15} /> : <Pause size={15} />}{" "}
                    {s.paused ? "Возобновить" : "Приостановить"}
                  </button>
                </>
              )}
            </div>
          </div>
          {error && (
            <div role="alert" className="error-banner">
              <TriangleAlert size={18} />
              <span>{error}</span>
              <button aria-label="Закрыть ошибку" onClick={() => setError("")}>
                <X size={16} />
              </button>
            </div>
          )}
          {query.isError && (
            <div role="alert" className="error-banner">
              <TriangleAlert size={20} />
              <span>{String(query.error)}</span>
            </div>
          )}
          {mutation.isPending && (
            <div role="status" className="pending">
              Выполняется запрос…
            </div>
          )}
          {!s ? (
            <Panel title="Desktop runtime">
              <Empty>
                <Cpu size={38} />
                <span>
                  {desktop
                    ? "Получение состояния фонового агента…"
                    : "Для работы с оборудованием запустите Tauri: npm run desktop:dev"}
                </span>
              </Empty>
            </Panel>
          ) : page === "Обзор" ? (
            <Overview s={s} act={act} />
          ) : page === "Устройства" ? (
            <Devices s={s} act={act} />
          ) : page === "Конвейер" ? (
            <Conveyor s={s} act={act} />
          ) : page === "Кнопки" ? (
            <Buttons s={s} act={act} />
          ) : page === "Подключения" ? (
            <Connections s={s} act={act} />
          ) : page === "Диагностика" ? (
            <Diagnostics s={s} act={act} onExport={onExport} />
          ) : (
            <SettingsPage s={s} act={act} onError={setError} />
          )}
          <footer>
            <span>
              <span className="footer-dot" /> DASTOR Gateway · Serial protocol
              v1
            </span>
            <span>
              {s?.simulation
                ? "SIMULATION · изолировано от production"
                : "Windows hardware agent"}{" "}
              <ExternalLink size={11} />
            </span>
          </footer>
        </div>
      </main>
      {quit && (
        <Confirm
          title="Завершить Gateway?"
          onClose={() => setQuit(false)}
          onConfirm={() => {
            void invokeQuit().catch((e) => setError(String(e)));
          }}
        >
          Агент перестанет отправлять heartbeat и принимать команды. При
          активном движении будет отправлен STOP, результат останется
          неопределённым до осмотра.
        </Confirm>
      )}
    </div>
  );
}
async function invokeQuit() {
  await invoke("quit_gateway");
}
