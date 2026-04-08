import { useState } from "react";
import type { AppState, LogEntry } from "@/lib/types";
import { cn } from "@/lib/utils";
import { CircleCheck, CircleX, Play, Square, RotateCw, Send } from "lucide-react";

interface Props {
  state: AppState;
}

type AdvancedTab = "logs" | "ddc" | "status";

function LogLine({ entry }: { entry: LogEntry }) {
  const levelColor = {
    trace: "text-text-tertiary",
    debug: "text-text-tertiary",
    info: "text-blue-400",
    warn: "text-warning",
    error: "text-danger",
  }[entry.level];

  return (
    <div className="flex gap-2 px-3 py-0.5 font-mono text-[10px] leading-5 hover:bg-surface-sunken/50 transition-colors">
      <span className="text-text-tertiary shrink-0 w-20">{entry.timestamp}</span>
      <span className={cn("shrink-0 w-10 uppercase font-semibold", levelColor)}>
        {entry.level}
      </span>
      <span className="text-text-tertiary shrink-0 w-24 truncate">{entry.source}</span>
      <span className="text-text-secondary">{entry.message}</span>
    </div>
  );
}

export function Advanced({ state }: Props) {
  const [tab, setTab] = useState<AdvancedTab>("logs");
  const [ddcMonitorId, setDdcMonitorId] = useState("");
  const [ddcValue, setDdcValue] = useState("");

  return (
    <div>
      <h2 className="text-[15px] font-semibold text-text mb-1">Advanced</h2>
      <p className="text-[12px] text-text-tertiary mb-6">
        Logs, DDC/CI history, connection diagnostics, and manual controls.
      </p>

      {/* sub-tabs */}
      <div className="flex gap-1 mb-4 p-0.5 bg-surface-sunken rounded-lg border border-border w-fit">
        {(["logs", "ddc", "status"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={cn(
              "px-3 py-1.5 rounded-md text-[11px] font-medium transition-all capitalize",
              tab === t
                ? "bg-surface-raised text-text shadow-sm"
                : "text-text-tertiary hover:text-text-secondary"
            )}
          >
            {t === "ddc" ? "DDC/CI" : t}
          </button>
        ))}
      </div>

      {/* logs tab */}
      {tab === "logs" && (
        <div className="border border-border rounded-lg overflow-hidden">
          <div className="bg-surface-sunken px-3 py-2 border-b border-border flex items-center justify-between">
            <span className="text-[10px] font-semibold tracking-widest text-text-tertiary uppercase">
              Live Log
            </span>
            <div className="flex gap-1">
              {["all", "info", "warn", "error"].map((level) => (
                <button
                  key={level}
                  className="px-2 py-0.5 rounded text-[10px] font-medium text-text-tertiary hover:text-text-secondary hover:bg-surface transition-colors capitalize"
                >
                  {level}
                </button>
              ))}
            </div>
          </div>
          <div className="bg-surface-sunken/30 max-h-80 overflow-y-auto py-1">
            {state.logs.map((entry, i) => (
              <LogLine key={i} entry={entry} />
            ))}
            {state.logs.length === 0 && (
              <div className="text-center py-8 text-[11px] text-text-tertiary">
                No log entries
              </div>
            )}
          </div>
        </div>
      )}

      {/* DDC history tab */}
      {tab === "ddc" && (
        <div>
          {/* manual DDC command */}
          <div className="border border-border rounded-lg p-4 bg-surface-raised mb-4">
            <div className="text-[10px] font-semibold tracking-widest text-text-tertiary uppercase mb-3">
              Manual DDC Command
            </div>
            <div className="flex items-end gap-2">
              <div className="flex-1">
                <label className="text-[10px] text-text-tertiary mb-1 block">Monitor ID</label>
                <input
                  type="text"
                  value={ddcMonitorId}
                  onChange={(e) => setDdcMonitorId(e.target.value)}
                  placeholder="DEL:U2720Q:SN12345"
                  className="w-full px-2.5 py-1.5 rounded-md border border-border bg-surface text-[11px] font-mono text-text placeholder:text-text-tertiary focus:outline-none focus:border-accent transition-colors"
                />
              </div>
              <div className="w-32">
                <label className="text-[10px] text-text-tertiary mb-1 block">VCP Value</label>
                <input
                  type="text"
                  value={ddcValue}
                  onChange={(e) => setDdcValue(e.target.value)}
                  placeholder="0x11"
                  className="w-full px-2.5 py-1.5 rounded-md border border-border bg-surface text-[11px] font-mono text-text placeholder:text-text-tertiary focus:outline-none focus:border-accent transition-colors"
                />
              </div>
              <button className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[11px] font-medium bg-accent text-surface hover:bg-accent-hover transition-colors">
                <Send size={11} />
                Execute
              </button>
            </div>
          </div>

          {/* command history */}
          <div className="border border-border rounded-lg overflow-hidden">
            <div className="bg-surface-sunken px-3 py-2 border-b border-border">
              <span className="text-[10px] font-semibold tracking-widest text-text-tertiary uppercase">
                Command History
              </span>
            </div>
            <table className="w-full text-[11px]">
              <thead>
                <tr className="border-b border-border bg-surface-sunken/50">
                  <th className="text-left px-3 py-1.5 font-medium text-text-tertiary">Time</th>
                  <th className="text-left px-3 py-1.5 font-medium text-text-tertiary">Monitor</th>
                  <th className="text-left px-3 py-1.5 font-medium text-text-tertiary">Command</th>
                  <th className="text-left px-3 py-1.5 font-medium text-text-tertiary">Value</th>
                  <th className="text-right px-3 py-1.5 font-medium text-text-tertiary">Duration</th>
                  <th className="w-8" />
                </tr>
              </thead>
              <tbody>
                {state.ddcHistory.map((cmd, i) => (
                  <tr key={i} className="border-b border-border last:border-0 hover:bg-surface-sunken/50 font-mono">
                    <td className="px-3 py-1.5 text-text-tertiary">{cmd.timestamp}</td>
                    <td className="px-3 py-1.5 text-text-secondary truncate max-w-32">{cmd.monitorId}</td>
                    <td className="px-3 py-1.5 text-text-secondary">{cmd.command}</td>
                    <td className="px-3 py-1.5 text-text">{cmd.value}</td>
                    <td className="px-3 py-1.5 text-text-tertiary text-right">{cmd.durationMs}ms</td>
                    <td className="px-2 py-1.5">
                      {cmd.success ? (
                        <CircleCheck size={12} className="text-success" />
                      ) : (
                        <CircleX size={12} className="text-danger" />
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* status tab */}
      {tab === "status" && (
        <div className="space-y-4">
          {/* deskflow status */}
          <div className="border border-border rounded-lg bg-surface-raised">
            <div className="px-4 py-3 border-b border-border flex items-center justify-between">
              <div>
                <div className="text-[12px] font-medium text-text">Deskflow Core</div>
                <div className="text-[10px] text-text-tertiary mt-0.5">
                  Keyboard, mouse, and clipboard sharing daemon
                </div>
              </div>
              <div className="flex items-center gap-2">
                <span
                  className={cn(
                    "flex items-center gap-1.5 text-[11px] font-medium",
                    state.deskflowStatus === "running"
                      ? "text-success"
                      : state.deskflowStatus === "restarting"
                        ? "text-warning"
                        : "text-danger"
                  )}
                >
                  <span
                    className={cn(
                      "w-1.5 h-1.5 rounded-full",
                      state.deskflowStatus === "running"
                        ? "bg-success"
                        : state.deskflowStatus === "restarting"
                          ? "bg-warning"
                          : "bg-danger"
                    )}
                  />
                  {state.deskflowStatus}
                </span>
              </div>
            </div>
            <div className="px-4 py-2.5 flex gap-2">
              {state.deskflowStatus === "running" ? (
                <button className="flex items-center gap-1.5 px-2.5 py-1 rounded text-[10px] font-medium text-danger bg-danger/5 border border-danger/20 hover:bg-danger/10 transition-colors">
                  <Square size={10} />
                  Stop
                </button>
              ) : (
                <button className="flex items-center gap-1.5 px-2.5 py-1 rounded text-[10px] font-medium text-success bg-success/5 border border-success/20 hover:bg-success/10 transition-colors">
                  <Play size={10} />
                  Start
                </button>
              )}
              <button className="flex items-center gap-1.5 px-2.5 py-1 rounded text-[10px] font-medium text-text-secondary bg-surface-sunken border border-border hover:border-text-tertiary transition-colors">
                <RotateCw size={10} />
                Restart
              </button>
            </div>
          </div>

          {/* machine connection health */}
          <div className="border border-border rounded-lg overflow-hidden">
            <div className="bg-surface-sunken px-4 py-2 border-b border-border">
              <span className="text-[10px] font-semibold tracking-widest text-text-tertiary uppercase">
                Connection Health
              </span>
            </div>
            {state.machines.map((m) => (
              <div
                key={m.name}
                className="flex items-center justify-between px-4 py-2.5 border-b border-border last:border-0"
              >
                <div className="flex items-center gap-2">
                  <span
                    className={cn(
                      "w-1.5 h-1.5 rounded-full",
                      m.online ? "bg-success" : "bg-danger"
                    )}
                  />
                  <span className="text-[12px] font-medium text-text">{m.name}</span>
                  <span className="text-[10px] text-text-tertiary capitalize">({m.role})</span>
                </div>
                <div className="flex items-center gap-4 text-[10px] text-text-tertiary font-mono">
                  <span>latency: {m.online ? "2ms" : "--"}</span>
                  <span>last heartbeat: {m.online ? "0s ago" : "disconnected"}</span>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
