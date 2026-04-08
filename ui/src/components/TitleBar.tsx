export function TitleBar() {
  const isMac = navigator.platform.toLowerCase().includes("mac");

  return (
    <div className="drag-region h-10 flex items-center border-b border-border shrink-0 bg-surface">
      {/* spacer for macOS traffic lights */}
      {isMac && <div className="w-20 shrink-0" />}
      <div className="flex-1 flex items-center justify-center">
        <span className="text-[11px] font-medium tracking-wide text-text-tertiary uppercase">
          full-kvm
        </span>
      </div>
      {/* spacer for Windows controls */}
      {!isMac && <div className="w-36 shrink-0" />}
    </div>
  );
}
