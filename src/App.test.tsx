import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App, { ButtonPad } from "./App";
import { statusSchema, type Status } from "./api";
import { useUi } from "./store";
const status: Status = {
  state: "READY",
  usbConnected: true,
  handshakeComplete: true,
  safe: true,
  firmware: "1.0.0",
  capabilities: ["BUTTONS", "CONVEYOR"],
  sessionId: "session",
  position: 1,
  activeCommand: null,
  fault: null,
  backendConnected: false,
  enrolled: false,
  deviceId: null,
  browserSessionId: null,
  lastHeartbeat: null,
  paused: true,
  simulation: true,
  statistics: {
    frames: 2,
    invalid: 0,
    duplicates: 0,
    dropped: 0,
    buttonEvents: 0,
    reconnects: 0,
  },
  buttons: [],
  logs: [],
  history: [],
  settings: {
    backendUrl: "https://localhost:8443",
    allowLoopbackHttp: false,
    port: null,
    baud: 115200,
    autoConnect: false,
    minimizeToTray: true,
    theme: "dark",
    language: "ru",
    stationMapping: [1, 2, 3, 4, 5, 6],
  },
};
beforeEach(() => useUi.setState({ page: "Обзор" }));
describe("hardware controls", () => {
  it("all six simulated buttons send typed protocol input", () => {
    const act = vi.fn();
    render(<ButtonPad status={status} act={act} />);
    for (const label of ["Вверх", "Вниз", "Влево", "Вправо", "Да", "Нет"])
      fireEvent.click(
        screen.getByRole("button", { name: `Simulation: ${label}` }),
      );
    expect(act.mock.calls.map((c) => c[0].control.button)).toEqual([
      "UP",
      "DOWN",
      "LEFT",
      "RIGHT",
      "YES",
      "NO",
    ]);
  });
  it("real hardware buttons cannot be synthetically pressed", () => {
    const act = vi.fn();
    render(<ButtonPad status={{ ...status, simulation: false }} act={act} />);
    const button = screen.getByRole("button", { name: "Да" });
    expect(button).toBeDisabled();
    fireEvent.click(button);
    expect(act).not.toHaveBeenCalled();
  });
  it("validates snapshots before they reach the UI", () => {
    expect(statusSchema.safeParse(status).success).toBe(true);
    expect(
      statusSchema.safeParse({ ...status, state: "SUCCESS_WITHOUT_SENSOR" })
        .success,
    ).toBe(false);
  });
  it("renders offline state without fabricated readings", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <App />
      </QueryClientProvider>,
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Веб-просмотр не подключён",
    );
    expect(screen.queryByText("Готов к работе")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Диагностика" }));
    expect(useUi.getState().page).toBe("Диагностика");
  });
});
