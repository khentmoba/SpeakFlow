import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { useTranslation } from "react-i18next";
import AppUpdatePanel from "@/components/settings/AppUpdatePanel";

export default function UpdatesPage({ animClass = "" }: { animClass?: string }) {
  const { t } = useTranslation();
  const [version, setVersion] = useState("");
  useEffect(() => { void getVersion().then(setVersion).catch(() => {}); }, []);
  return (
    <div className="page-root">
      <div className={`page-content updates-content ${animClass}`}>
        <header className="workspace-heading"><div>
          <h1>{t("updates.heading")}</h1>
          <p>{t("updates.subheading")}</p>
        </div></header>
        <AppUpdatePanel appVersion={version} />
      </div>
    </div>
  );
}
