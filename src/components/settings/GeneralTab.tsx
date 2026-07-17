import React from "react"
import { GeneralSettings, NewTabServerSort } from "../../types"
import { useTranslation } from "../../i18n"
import { useConfig } from "../../hooks/useConfig"
import { CustomSelect } from "../CustomSelect"

const MIN_FIXED_TAB_WIDTH = 120
const MAX_FIXED_TAB_WIDTH = 400
const DEFAULT_FIXED_TAB_WIDTH = 200

export interface GeneralTabProps {
  general: GeneralSettings
  onGeneralUpdate: (general: GeneralSettings) => void
}

export const GeneralTab: React.FC<GeneralTabProps> = ({
  general,
  onGeneralUpdate,
}) => {
  const { t } = useTranslation()
  const { config } = useConfig()

  const updateSettings = general.update ?? { autoCheck: true, proxyId: null }
  const proxyOptions = config?.proxies ?? []
  const selectedProxyId = updateSettings.proxyId || ""
  const selectedProxyExists =
    !selectedProxyId ||
    proxyOptions.some((proxy) => proxy.id === selectedProxyId)
  const effectiveProxyId = selectedProxyExists ? selectedProxyId : ""

  const handleThemeChange = (
    theme: "light" | "dark" | "orange" | "green" | "system",
  ) => {
    onGeneralUpdate({ ...general, theme })
  }

  const handleLanguageChange = (language: "en" | "zh-CN") => {
    onGeneralUpdate({ ...general, language })
  }

  const handleRecordingModeChange = (recordingMode: "raw" | "text") => {
    onGeneralUpdate({ ...general, recordingMode })
  }

  const handleTabWidthModeChange = (tabWidthMode: "adaptive" | "fixed") => {
    onGeneralUpdate({ ...general, tabWidthMode })
  }

  const handleFixedTabWidthChange = (tabFixedWidth: number) => {
    const normalizedWidth = Number.isFinite(tabFixedWidth)
      ? tabFixedWidth
      : DEFAULT_FIXED_TAB_WIDTH
    onGeneralUpdate({ ...general, tabFixedWidth: normalizedWidth })
  }

  const handleTerminalRightClickModeChange = (
    terminalRightClickMode: "contextMenu" | "selectionCopyPaste",
  ) => {
    onGeneralUpdate({ ...general, terminalRightClickMode })
  }

  const handleTabNewServerSortChange = (tabNewServerSort: NewTabServerSort) => {
    onGeneralUpdate({ ...general, tabNewServerSort })
  }

  const handleTerminalUpdate = (
    field: keyof typeof general.terminal,
    value: string | number,
  ) => {
    onGeneralUpdate({
      ...general,
      terminal: { ...general.terminal, [field]: value },
    })
  }

  const handleConfirmationChange = (
    field:
      | "confirmCloseTab"
      | "confirmExitApp"
      | "debugEnabled"
      | "terminalCommandBlockBar"
      | "maxRecentServers",
    value: boolean | number,
  ) => {
    onGeneralUpdate({ ...general, [field]: value })
  }

  const handleUpdateSettingsChange = (patch: {
    autoCheck?: boolean
    proxyId?: string | null
  }) => {
    onGeneralUpdate({
      ...general,
      update: {
        autoCheck: patch.autoCheck ?? updateSettings.autoCheck ?? true,
        proxyId:
          patch.proxyId !== undefined
            ? patch.proxyId
            : (updateSettings.proxyId ?? null),
      },
    })
  }

  return (
    <div className="w-full max-w-full space-y-6">
      {/* Appearance Section */}
      <div>
        <h3 className="text-base font-semibold  mb-4">{t.appearance}</h3>
        <div className="space-y-4">
          <div className="flex flex-col gap-1.5 mb-4">
            <label
              htmlFor="theme-select"
              className="block text-sm font-medium text-zinc-400 mb-1.5 "
            >
              {t.theme}
            </label>
            <CustomSelect
              id="theme-select"
              value={general.theme}
              onChange={(val) =>
                handleThemeChange(
                  val as "light" | "dark" | "orange" | "green" | "system",
                )
              }
              options={[
                { value: "system", label: t.system },
                { value: "light", label: t.light },
                { value: "dark", label: t.dark },
                { value: "orange", label: t.orange },
                { value: "green", label: t.green },
              ]}
            />
          </div>

          <div className="flex flex-col gap-1.5 mb-4">
            <label
              htmlFor="language-select"
              className="block text-sm font-medium text-zinc-400 mb-1.5 "
            >
              {t.language}
            </label>
            <CustomSelect
              id="language-select"
              value={general.language}
              onChange={(val) => handleLanguageChange(val as "en" | "zh-CN")}
              options={[
                { value: "en", label: "English" },
                { value: "zh-CN", label: "简体中文" },
              ]}
            />
          </div>

          <div className="flex flex-col gap-1.5 mb-4">
            <label
              htmlFor="recording-mode-select"
              className="block text-sm font-medium text-zinc-400 mb-1.5 "
            >
              {t.recordingMode}
            </label>
            <CustomSelect
              id="recording-mode-select"
              value={general.recordingMode || "raw"}
              onChange={(val) =>
                handleRecordingModeChange(val as "raw" | "text")
              }
              options={[
                { value: "raw", label: t.recordingModes.raw },
                { value: "text", label: t.recordingModes.text },
              ]}
            />
          </div>

          <div className="flex flex-col gap-1.5 mb-4">
            <label
              htmlFor="max-recent-servers"
              className="block text-sm font-medium text-zinc-400 mb-1.5 "
            >
              {t.maxRecentServers}
            </label>
            <input
              id="max-recent-servers"
              type="number"
              value={general.maxRecentServers}
              onChange={(e) =>
                handleConfirmationChange(
                  "maxRecentServers",
                  parseInt(e.target.value) || 0,
                )
              }
              min="0"
              max="20"
              className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
            />
          </div>

          <div className="flex flex-col gap-1.5 mb-4">
            <label
              htmlFor="tab-width-mode-select"
              className="block text-sm font-medium text-zinc-400 mb-1.5 "
            >
              {t.tabWidthMode}
            </label>
            <CustomSelect
              id="tab-width-mode-select"
              value={general.tabWidthMode || "fixed"}
              onChange={(val) =>
                handleTabWidthModeChange(val as "adaptive" | "fixed")
              }
              options={[
                { value: "adaptive", label: t.tabWidthModes.adaptive },
                { value: "fixed", label: t.tabWidthModes.fixed },
              ]}
            />
          </div>

          {(general.tabWidthMode || "fixed") === "fixed" && (
            <div className="flex flex-col gap-1.5 mb-4">
              <label
                htmlFor="fixed-tab-width"
                className="block text-sm font-medium text-zinc-400 mb-1.5 "
              >
                {t.fixedTabWidth}
              </label>
              <input
                id="fixed-tab-width"
                type="number"
                value={general.tabFixedWidth || DEFAULT_FIXED_TAB_WIDTH}
                onChange={(e) =>
                  handleFixedTabWidthChange(
                    parseInt(e.target.value, 10) || DEFAULT_FIXED_TAB_WIDTH,
                  )
                }
                onBlur={() =>
                  handleFixedTabWidthChange(
                    Math.max(
                      MIN_FIXED_TAB_WIDTH,
                      Math.min(
                        MAX_FIXED_TAB_WIDTH,
                        general.tabFixedWidth || DEFAULT_FIXED_TAB_WIDTH,
                      ),
                    ),
                  )
                }
                min={MIN_FIXED_TAB_WIDTH}
                max={MAX_FIXED_TAB_WIDTH}
                className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
              />
              <span className="text-xs text-[var(--text-muted)]">
                {t.fixedTabWidthHint
                  .replace("{min}", MIN_FIXED_TAB_WIDTH.toString())
                  .replace("{max}", MAX_FIXED_TAB_WIDTH.toString())}
              </span>
            </div>
          )}

          <div className="flex flex-col gap-1.5 mb-4">
            <label
              htmlFor="terminal-right-click-mode-select"
              className="block text-sm font-medium text-zinc-400 mb-1.5 "
            >
              {t.terminalRightClickMode}
            </label>
            <CustomSelect
              id="terminal-right-click-mode-select"
              value={general.terminalRightClickMode || "contextMenu"}
              onChange={(val) =>
                handleTerminalRightClickModeChange(
                  val as "contextMenu" | "selectionCopyPaste",
                )
              }
              options={[
                {
                  value: "contextMenu",
                  label: t.terminalRightClickModes.contextMenu,
                },
                {
                  value: "selectionCopyPaste",
                  label: t.terminalRightClickModes.selectionCopyPaste,
                },
              ]}
            />
          </div>

          <div className="flex flex-col gap-1.5 mb-4">
            <label
              htmlFor="tab-new-server-sort-select"
              className="block text-sm font-medium text-zinc-400 mb-1.5 "
            >
              {t.tabNewServerSort}
            </label>
            <CustomSelect
              id="tab-new-server-sort-select"
              value={general.tabNewServerSort || "default"}
              onChange={(val) =>
                handleTabNewServerSortChange(val as NewTabServerSort)
              }
              options={[
                { value: "default", label: t.tabNewServerSorts.default },
                { value: "recent", label: t.tabNewServerSorts.recent },
                {
                  value: "connectionCount",
                  label: t.tabNewServerSorts.connectionCount,
                },
                { value: "createdAt", label: t.tabNewServerSorts.createdAt },
                { value: "updatedAt", label: t.tabNewServerSorts.updatedAt },
              ]}
            />
          </div>
        </div>
      </div>

      {/* Software Update Section */}
      <div>
        <h3 className="text-base font-semibold  mb-4">{t.softwareUpdate}</h3>
        <div className="space-y-4">
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={updateSettings.autoCheck ?? true}
              onChange={(e) =>
                handleUpdateSettingsChange({ autoCheck: e.target.checked })
              }
              className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed"
            />
            <span className="block text-sm font-medium text-zinc-400 mb-0 ">
              {t.updateAutoCheck}
            </span>
          </label>
          <p className="text-xs text-[var(--text-muted)] -mt-2 ml-7">
            {t.updateAutoCheckHint}
          </p>

          <div className="flex flex-col gap-1.5 mb-1">
            <label
              htmlFor="update-proxy-select"
              className="block text-sm font-medium text-zinc-400 mb-1.5 "
            >
              {t.updateProxy}
            </label>
            <CustomSelect
              id="update-proxy-select"
              value={effectiveProxyId}
              onChange={(val) =>
                handleUpdateSettingsChange({
                  proxyId: val ? val : null,
                })
              }
              options={[
                { value: "", label: t.common.noProxy },
                ...proxyOptions.map((proxy) => ({
                  value: proxy.id,
                  label: proxy.name,
                })),
              ]}
            />
            {!selectedProxyExists && selectedProxyId ? (
              <span className="text-xs text-amber-400">
                {t.updateProxyMissing}
              </span>
            ) : (
              <span className="text-xs text-[var(--text-muted)]">
                {t.updateProxyHint}
              </span>
            )}
          </div>
        </div>
      </div>

      {/* Terminal Settings Section */}
      <div>
        <h3 className="text-base font-semibold  mb-4">{t.terminal}</h3>
        <div className="space-y-4">
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={general.terminalCommandBlockBar ?? true}
              onChange={(e) =>
                handleConfirmationChange(
                  "terminalCommandBlockBar",
                  e.target.checked,
                )
              }
              className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed"
            />
            <span className="block text-sm font-medium text-zinc-400 mb-0 ">
              {t.terminalCommandBlockBar}
            </span>
          </label>

          <div className="flex flex-col gap-1.5 mb-4">
            <label
              htmlFor="font-family"
              className="block text-sm font-medium text-zinc-400 mb-1.5 "
            >
              {t.fontFamily}
            </label>
            <input
              id="font-family"
              type="text"
              value={general.terminal.fontFamily}
              onChange={(e) =>
                handleTerminalUpdate("fontFamily", e.target.value)
              }
              className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
              placeholder={t.fontFamilyPlaceholder}
            />
          </div>

          <div className="flex flex-col gap-1.5 mb-4">
            <label
              htmlFor="font-size"
              className="block text-sm font-medium text-zinc-400 mb-1.5 "
            >
              {t.fontSize}
            </label>
            <input
              id="font-size"
              type="number"
              value={general.terminal.fontSize}
              onChange={(e) =>
                handleTerminalUpdate("fontSize", parseInt(e.target.value) || 14)
              }
              min="8"
              max="32"
              className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
            />
          </div>

          <div className="flex flex-col gap-1.5 mb-4">
            <label
              htmlFor="cursor-style"
              className="block text-sm font-medium text-zinc-400 mb-1.5 "
            >
              {t.cursorStyle}
            </label>
            <CustomSelect
              id="cursor-style"
              value={general.terminal.cursorStyle}
              onChange={(val) => handleTerminalUpdate("cursorStyle", val)}
              options={[
                { value: "block", label: t.cursorStyles.block },
                { value: "underline", label: t.cursorStyles.underline },
                { value: "bar", label: t.cursorStyles.bar },
              ]}
            />
          </div>

          <div className="flex flex-col gap-1.5 mb-4">
            <label
              htmlFor="scrollback-limit"
              className="block text-sm font-medium text-zinc-400 mb-1.5 "
            >
              {t.scrollback}
            </label>
            <input
              id="scrollback-limit"
              type="number"
              value={general.terminal.scrollback}
              onChange={(e) =>
                handleTerminalUpdate(
                  "scrollback",
                  parseInt(e.target.value) || 1000,
                )
              }
              min="100"
              max="50000"
              className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
            />
          </div>
        </div>
      </div>

      {/* Confirmations Section */}
      <div>
        <h3 className="text-base font-semibold  mb-4">{t.confirmations}</h3>
        <div className="space-y-3">
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={general.confirmCloseTab}
              onChange={(e) =>
                handleConfirmationChange("confirmCloseTab", e.target.checked)
              }
              className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed"
            />
            <span className="block text-sm font-medium text-zinc-400 mb-0 ">
              {t.confirmCloseTab}
            </span>
          </label>

          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={general.confirmExitApp}
              onChange={(e) =>
                handleConfirmationChange("confirmExitApp", e.target.checked)
              }
              className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed"
            />
            <span className="block text-sm font-medium text-zinc-400 mb-0 ">
              {t.confirmExitApp}
            </span>
          </label>

          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={general.debugEnabled}
              onChange={(e) =>
                handleConfirmationChange("debugEnabled", e.target.checked)
              }
              className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed"
            />
            <span className="block text-sm font-medium text-zinc-400 mb-0 ">
              {t.debugEnabled}
            </span>
          </label>
        </div>
      </div>
    </div>
  )
}
