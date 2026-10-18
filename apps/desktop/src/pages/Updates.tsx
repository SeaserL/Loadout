// SPDX-License-Identifier: Apache-2.0
import { api, call } from "../api";
import { Button, Card, ErrorText, Notice, Switch, useConfirm } from "../components";
import { useAction, usePoll } from "../hooks";
import { currentLang } from "../i18n";

type UpdateStatus = {
  current_version: string; latest_version: string | null; phase: string;
  checked_at_ms: number | null; error: string | null; install_supported: boolean;
};
const status = () => call<UpdateStatus>("updates.status");
export function Updates() {
  const update = usePoll(status);
  const settings = usePoll(api.settings);
  const action = useAction();
  const confirm = useConfirm();
  const zh = currentLang() === "zh-CN";
  const text = (cn: string, en: string) => zh ? cn : en;
  const phases: Record<string, string> = {
    idle: text("尚未检查", "Not checked"), checking: text("正在检查", "Checking"),
    current: text("已是最新正式版", "Up to date"), available: text("发现新版本", "Update available"),
    downloading: text("正在下载并校验", "Downloading and verifying"), ready: text("已校验，可以安装", "Verified and ready"),
    installing: text("正在安装，将重启应用", "Installing; the app will restart"), error: text("更新失败", "Update failed"),
  };
  const busy = action.busy || ["checking", "downloading", "installing"].includes(update.data?.phase ?? "");
  const restart = text("安装会排空并重启网关，短暂中断 API 服务；失败时恢复旧程序。", "Installation drains and restarts the gateway, briefly interrupting API service. Failures restore the previous programs.");
  return <Card title={text("软件更新", "Software updates")}>
    <p>Loadout {update.data?.current_version} · {phases[update.data?.phase ?? "idle"]}</p>
    {update.data?.latest_version && <p>{text("最新正式版：", "Latest stable release: ")}{update.data.latest_version}</p>}
    {update.data?.checked_at_ms && <p className="small muted">{new Date(update.data.checked_at_ms).toLocaleString()}</p>}
    <p className="small muted">{text("从 SeaserL/Loadout 的 GitHub Releases 检查正式版本，每六小时一次；下载后校验 SHA-256。", "Checks stable GitHub Releases from SeaserL/Loadout every six hours; verifies SHA-256 after downloading.")}</p>
    <p className="small muted">{text("私有仓库使用本机 Git Credential Manager 已有的 GitHub 登录；账号需要仓库读取权限。不会弹出登录或保存新的凭据。", "Private releases use your existing GitHub login in Git Credential Manager, with repository read access. No sign-in prompts or new credentials are stored.")}</p>
    {([ ["updates.enabled", text("自动检查", "Check automatically")], ["updates.auto_download", text("自动下载", "Download automatically")], ["updates.auto_install", text("自动安装并重启", "Install and restart automatically")] ] as [string, string][]).map(([key, label]) => <div className="row" key={key} style={{marginBottom: 12}}>
      <span>{label}</span><Switch label={label} checked={settings.data?.find(s => s.key === key)?.value === "true"} disabled={action.busy} onChange={on => void action.run(async () => {
        if (key === "updates.auto_install" && on && !await confirm({title: label, message: restart, confirmLabel: text("开启", "Enable")})) return;
        await api.setSetting(key, String(on)); await settings.reload();
      })} />
    </div>)}
    {!update.data?.install_supported && <Notice>{text("开发版或非标准安装目录：可下载，不能自动覆盖。请使用发布包安装到默认目录。", "Development or nonstandard installation: downloads are supported, automatic replacement is disabled. Install the release package to its default directory.")}</Notice>}
    <p className="small muted">{restart}</p>
    <div className="row">
      <Button disabled={busy} onClick={() => void action.run(async () => {await call("updates.check"); await update.reload();})}>{text("立即检查并下载", "Check and download")}</Button>
      <Button disabled={busy || update.data?.phase !== "ready" || !update.data.install_supported} onClick={() => void action.run(async () => {
        if (await confirm({title: text("安装更新", "Install update"), message: restart, confirmLabel: text("安装并重启", "Install and restart")})) {await call("updates.install"); await update.reload();}
      })}>{text("安装并重启", "Install and restart")}</Button>
    </div>
    <ErrorText error={action.error ?? update.error?.message} />
    {update.data?.error && <Notice>{update.data.error}</Notice>}
  </Card>;
}
