import { useState } from "react";
import type { AppState } from "@/lib/types";
import { cn, renderShortcut } from "@/lib/utils";

interface Props {
  state: AppState;
}

function KeyCap({ label }: { label: string }) {
  return (
    <span className="inline-flex items-center justify-center min-w-[24px] h-6 px-1.5 rounded bg-surface-sunken border border-border text-[11px] font-mono font-medium text-text-secondary shadow-[0_1px_0_0] shadow-border">
      {label}
    </span>
  );
}

function Toggle({ enabled }: { enabled: boolean }) {
  return (
    <button
      className={cn(
        "relative w-9 h-5 rounded-full transition-colors",
        enabled ? "bg-success" : "bg-border"
      )}
    >
      <span
        className={cn(
          "absolute top-0.5 w-4 h-4 rounded-full bg-white shadow transition-transform",
          enabled ? "translate-x-4" : "translate-x-0.5"
        )}
      />
    </button>
  );
}

function SettingRow({
  label,
  description,
  children,
}: {
  label: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between py-3 px-4 border-b border-border last:border-0">
      <div className="min-w-0 mr-4">
        <div className="text-[12px] font-medium text-text">{label}</div>
        {description && (
          <div className="text-[10px] text-text-tertiary mt-0.5">{description}</div>
        )}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

export function Behavior({ state }: Props) {
  const { behavior } = state;
  const [delay, setDelay] = useState(behavior.switchDelay);

  return (
    <div>
      <h2 className="text-[15px] font-semibold text-text mb-1">Behavior</h2>
      <p className="text-[12px] text-text-tertiary mb-6">
        Switch timing, hotkeys, clipboard, and notification preferences.
      </p>

      {/* switch delay */}
      <div className="mb-6">
        <div className="text-[10px] font-semibold tracking-widest text-text-tertiary uppercase mb-3">
          Switch Timing
        </div>
        <div className="border border-border rounded-lg bg-surface-raised">
          <SettingRow
            label="Edge switch delay"
            description="Milliseconds the cursor must stay at the screen edge before switching"
          >
            <div className="flex items-center gap-3">
              <input
                type="range"
                min={50}
                max={1000}
                step={25}
                value={delay}
                onChange={(e) => setDelay(Number(e.target.value))}
                className="w-28 accent-accent"
              />
              <span className="text-[11px] font-mono text-text-secondary w-12 text-right">
                {delay}ms
              </span>
            </div>
          </SettingRow>
          <SettingRow
            label="Adaptive switch delay"
            description="Automatically adjust delay based on switching frequency"
          >
            <Toggle enabled={behavior.adaptiveSwitchDelay} />
          </SettingRow>
          <SettingRow
            label="Idle timeout"
            description="Minutes before a machine is considered idle"
          >
            <span className="text-[11px] font-mono text-text-secondary">
              {behavior.idleTimeoutMin} min
            </span>
          </SettingRow>
        </div>
      </div>

      {/* hotkeys */}
      <div className="mb-6">
        <div className="text-[10px] font-semibold tracking-widest text-text-tertiary uppercase mb-3">
          Hotkeys
        </div>
        <div className="border border-border rounded-lg bg-surface-raised">
          <SettingRow label="Focus lock" description="Lock input to the current machine">
            <span className="inline-flex items-center gap-0.5">
              {renderShortcut(behavior.focusLockHotkey).map((k, i) => (
                <KeyCap key={i} label={k} />
              ))}
            </span>
          </SettingRow>
          <SettingRow label="Quick switch" description="Switch to next machine instantly">
            <span className="inline-flex items-center gap-0.5">
              {renderShortcut(behavior.quickSwitchHotkey).map((k, i) => (
                <KeyCap key={i} label={k} />
              ))}
            </span>
          </SettingRow>
          <SettingRow label="Quick switch back" description="Switch to previous machine">
            <span className="inline-flex items-center gap-0.5">
              {renderShortcut(behavior.quickSwitchBackHotkey).map((k, i) => (
                <KeyCap key={i} label={k} />
              ))}
            </span>
          </SettingRow>
        </div>
      </div>

      {/* clipboard and notifications */}
      <div>
        <div className="text-[10px] font-semibold tracking-widest text-text-tertiary uppercase mb-3">
          Clipboard & Notifications
        </div>
        <div className="border border-border rounded-lg bg-surface-raised">
          <SettingRow
            label="Clipboard sharing"
            description="Share clipboard content between machines"
          >
            <Toggle enabled={behavior.clipboardSharing} />
          </SettingRow>
          <SettingRow
            label="Max clipboard size"
            description="Maximum size for shared clipboard data"
          >
            <span className="text-[11px] font-mono text-text-secondary">
              {behavior.clipboardMaxSizeKb} KB
            </span>
          </SettingRow>
          <SettingRow
            label="Toast notifications"
            description="Show overlay when switching machines"
          >
            <Toggle enabled={behavior.toastNotifications} />
          </SettingRow>
          <SettingRow
            label="Toast duration"
          >
            <span className="text-[11px] font-mono text-text-secondary">
              {behavior.toastDurationMs}ms
            </span>
          </SettingRow>
        </div>
      </div>
    </div>
  );
}
