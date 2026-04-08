import type { AppState } from "@/lib/types";
import { cn, renderShortcut } from "@/lib/utils";
import { ArrowRight, Check, X } from "lucide-react";

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

function ShortcutDisplay({ shortcut }: { shortcut: string }) {
  const keys = renderShortcut(shortcut);
  return (
    <span className="inline-flex items-center gap-0.5">
      {keys.map((k, i) => (
        <KeyCap key={i} label={k} />
      ))}
    </span>
  );
}

export function Keyboard({ state }: Props) {
  const { autoRemap, translations } = state.keyboard;

  return (
    <div>
      <h2 className="text-[15px] font-semibold text-text mb-1">Keyboard Remapping</h2>
      <p className="text-[12px] text-text-tertiary mb-6">
        Automatic modifier translation and shortcut mappings between OS pairs.
      </p>

      {/* auto-remap toggle */}
      <div className="flex items-center justify-between px-4 py-3 rounded-lg border border-border bg-surface-raised mb-6">
        <div>
          <div className="text-[12px] font-medium text-text">Automatic modifier remapping</div>
          <div className="text-[10px] text-text-tertiary mt-0.5">
            Cmd\u2194Ctrl translation on cross-OS screen switches
          </div>
        </div>
        <button
          className={cn(
            "relative w-9 h-5 rounded-full transition-colors",
            autoRemap ? "bg-success" : "bg-border"
          )}
        >
          <span
            className={cn(
              "absolute top-0.5 w-4 h-4 rounded-full bg-white shadow transition-transform",
              autoRemap ? "translate-x-4" : "translate-x-0.5"
            )}
          />
        </button>
      </div>

      {/* modifier mapping diagram */}
      <div className="mb-6">
        <div className="text-[10px] font-semibold tracking-widest text-text-tertiary uppercase mb-3">
          Modifier Mapping
        </div>
        <div className="grid grid-cols-2 gap-4">
          {/* mac -> windows */}
          <div className="p-4 rounded-lg border border-border bg-surface-raised">
            <div className="text-[11px] font-medium text-text-secondary mb-3">
              macOS keyboard \u2192 Windows screen
            </div>
            <div className="space-y-2">
              {[
                ["\u2318 Command", "\u2192", "Ctrl"],
                ["\u2303 Control", "\u2192", "Win"],
                ["\u2325 Option", "\u2192", "Alt"],
              ].map(([from, arrow, to]) => (
                <div key={from} className="flex items-center gap-2 text-[11px]">
                  <span className="font-mono text-text-secondary w-28">{from}</span>
                  <span className="text-text-tertiary">{arrow}</span>
                  <span className="font-mono font-medium text-text">{to}</span>
                </div>
              ))}
            </div>
          </div>
          {/* windows -> mac */}
          <div className="p-4 rounded-lg border border-border bg-surface-raised">
            <div className="text-[11px] font-medium text-text-secondary mb-3">
              Windows keyboard \u2192 macOS screen
            </div>
            <div className="space-y-2">
              {[
                ["Ctrl", "\u2192", "\u2318 Command"],
                ["Win", "\u2192", "\u2303 Control"],
                ["Alt", "\u2192", "\u2325 Option"],
              ].map(([from, arrow, to]) => (
                <div key={from} className="flex items-center gap-2 text-[11px]">
                  <span className="font-mono text-text-secondary w-28">{from}</span>
                  <span className="text-text-tertiary">{arrow}</span>
                  <span className="font-mono font-medium text-text">{to}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>

      {/* shortcut translations */}
      <div>
        <div className="text-[10px] font-semibold tracking-widest text-text-tertiary uppercase mb-3">
          Shortcut Translations
        </div>
        <div className="border border-border rounded-lg overflow-hidden">
          <table className="w-full text-[12px]">
            <thead>
              <tr className="bg-surface-sunken border-b border-border">
                <th className="text-left px-4 py-2 font-medium text-text-tertiary">Intent</th>
                <th className="text-left px-4 py-2 font-medium text-text-tertiary">macOS</th>
                <th className="w-8" />
                <th className="text-left px-4 py-2 font-medium text-text-tertiary">Windows</th>
                <th className="text-center px-4 py-2 font-medium text-text-tertiary w-16">Active</th>
              </tr>
            </thead>
            <tbody>
              {translations.map((t) => (
                <tr key={t.intent} className="border-b border-border last:border-0 hover:bg-surface-sunken/50 transition-colors">
                  <td className="px-4 py-2.5 font-medium text-text">
                    {t.intent.replace(/_/g, " ")}
                  </td>
                  <td className="px-4 py-2.5">
                    <ShortcutDisplay shortcut={t.mac} />
                  </td>
                  <td className="text-center text-text-tertiary">
                    <ArrowRight size={12} />
                  </td>
                  <td className="px-4 py-2.5">
                    <ShortcutDisplay shortcut={t.windows} />
                  </td>
                  <td className="px-4 py-2.5 text-center">
                    {t.enabled ? (
                      <Check size={14} className="mx-auto text-success" />
                    ) : (
                      <X size={14} className="mx-auto text-text-tertiary" />
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
