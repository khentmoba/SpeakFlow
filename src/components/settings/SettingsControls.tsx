import type { ReactNode } from "react";

interface SettingsToggleProps {
  checked: boolean;
  label: string;
  disabled?: boolean;
  onChange: (next: boolean) => void;
}

/** The SIGNAL switch. Always labelled: the row text is not a <label>. */
export function SettingsToggle({ checked, label, disabled = false, onChange }: SettingsToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className="toggle-switch"
      data-active={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    >
      <div className="toggle-knob" />
    </button>
  );
}

interface SettingsToggleRowProps {
  label: string;
  hint?: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (next: boolean) => void;
}

export function SettingsToggleRow({
  label,
  hint,
  checked,
  disabled = false,
  onChange,
}: SettingsToggleRowProps) {
  return (
    <div className="settings-row">
      <div className="settings-toggle-copy">
        <span className="settings-option-label">{label}</span>
        {hint && <span className="settings-option-desc">{hint}</span>}
      </div>
      <SettingsToggle checked={checked} label={label} disabled={disabled} onChange={onChange} />
    </div>
  );
}

interface SettingsSelectRowProps<T extends string> {
  label: string;
  hint?: string;
  value: T;
  options: readonly { value: T; label: string }[];
  disabled?: boolean;
  onChange: (next: T) => void;
}

export function SettingsSelectRow<T extends string>({
  label,
  hint,
  value,
  options,
  disabled = false,
  onChange,
}: SettingsSelectRowProps<T>) {
  return (
    <div className="settings-row">
      <div className="settings-toggle-copy">
        <span className="settings-option-label">{label}</span>
        {hint && <span className="settings-option-desc">{hint}</span>}
      </div>
      <select
        className="settings-input app-rule-select"
        aria-label={label}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value as T)}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>{option.label}</option>
        ))}
      </select>
    </div>
  );
}

interface SettingsRangeRowProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  display: string;
  disabled?: boolean;
  onChange: (next: number) => void;
}

export function SettingsRangeRow({
  label,
  value,
  min,
  max,
  step,
  display,
  disabled = false,
  onChange,
}: SettingsRangeRowProps) {
  return (
    <div className="settings-row">
      <div className="settings-toggle-copy settings-range-row">
        <span className="settings-option-label">{label}</span>
        <input
          type="range"
          aria-label={label}
          value={value}
          min={min}
          max={max}
          step={step}
          disabled={disabled}
          onChange={(event) => onChange(Number(event.target.value))}
        />
      </div>
      <span className="settings-option-desc">{display}</span>
    </div>
  );
}

export function SettingsSectionHeader({ icon, title, trailing }: {
  icon: ReactNode;
  title: string;
  trailing?: ReactNode;
}) {
  return (
    <div className="settings-section-header">
      {icon}
      <h2 className="settings-section-title">{title}</h2>
      {trailing && <div className="settings-header-actions">{trailing}</div>}
    </div>
  );
}
