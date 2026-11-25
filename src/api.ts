import { invoke, isTauri } from "@tauri-apps/api/core";
import { z } from "zod";
const nullableString = z.string().nullable();
export const settingsSchema = z.object({
  backendUrl: z.string(),
  allowLoopbackHttp: z.boolean(),
  port: nullableString,
  baud: z.number(),
  autoConnect: z.boolean(),
  minimizeToTray: z.boolean(),
  theme: z.enum(["dark", "light"]),
  language: z.literal("ru"),
  stationMapping: z.array(z.number()).length(6),
});
export const buttonSchema = z.object({
  eventId: z.string(),
  deviceId: z.string(),
  sessionId: z.string(),
  seq: z.number(),
  button: z.enum(["UP", "DOWN", "LEFT", "RIGHT", "YES", "NO"]),
  edge: z.enum(["PRESS", "RELEASE"]),
  occurredAt: z.string(),
  expiresAt: z.string(),
  browserSessionId: nullableString,
  simulation: z.boolean(),
});
export const statusSchema = z.object({
  state: z.enum([
    "DISCONNECTED",
    "INITIALIZING",
    "READY",
    "MOVING",
    "ARRIVED",
    "FAULT",
    "E_STOP",
    "NEEDS_INSPECTION",
  ]),
  usbConnected: z.boolean(),
  handshakeComplete: z.boolean(),
  safe: z.boolean(),
  firmware: nullableString,
  capabilities: z.array(z.string()),
  sessionId: z.string(),
  position: z.number().nullable(),
  activeCommand: nullableString,
  fault: nullableString,
  backendConnected: z.boolean(),
  enrolled: z.boolean(),
  deviceId: nullableString,
  browserSessionId: nullableString,
  lastHeartbeat: nullableString,
  paused: z.boolean(),
  simulation: z.boolean(),
  statistics: z.object({
    frames: z.number(),
    invalid: z.number(),
    duplicates: z.number(),
    dropped: z.number(),
    buttonEvents: z.number(),
    reconnects: z.number(),
  }),
  buttons: z.array(buttonSchema),
  logs: z.array(
    z.object({ at: z.string(), level: z.string(), message: z.string() }),
  ),
  history: z.array(
    z.object({
      eventId: z.string(),
      commandId: z.string(),
      kind: z.string(),
      occurredAt: z.string(),
    }),
  ),
  settings: settingsSchema,
});
export type Status = z.infer<typeof statusSchema>;
export type Settings = z.infer<typeof settingsSchema>;
export type ButtonName = z.infer<typeof buttonSchema>["button"];
export type Port = {
  name: string;
  vid: number | null;
  pid: number | null;
  serialNumber: string | null;
  manufacturer: string | null;
  product: string | null;
};
export const desktop = isTauri();
export async function getStatus(): Promise<Status> {
  if (!desktop)
    throw new Error(
      "Откройте desktop-приложение: npm run desktop:dev. Веб-просмотр не подключён к оборудованию.",
    );
  return statusSchema.parse(await invoke("gateway_status"));
}
export async function execute(action: Record<string, unknown>) {
  if (!desktop) throw new Error("Нужен desktop runtime Tauri");
  await invoke("gateway_action", { action });
}
export async function getPorts(): Promise<Port[]> {
  return invoke("serial_ports");
}
export async function exportDiagnostics(): Promise<string> {
  return invoke("export_diagnostics");
}
export async function getAutostart(): Promise<boolean> {
  return invoke("get_autostart");
}
export async function setAutostart(enabled: boolean) {
  await invoke("set_autostart", { enabled });
}
