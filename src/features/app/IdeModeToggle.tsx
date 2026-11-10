import { executeAction } from "@/platform/actions";
import { ideMode } from "@/platform/editorSettings";
import { useTx } from "@/platform/i18n";
import { shortcutLabel } from "@/platform/keymap";
import { useProject } from "@/platform/project";
import { BarButton, Icon } from "@/ui";
import styles from "./IdeModeToggle.module.css";

/** Title bar: the IDE mode of the window's project; a click switches it. */
export function IdeModeToggle() {
  const tx = useTx();
  const mode = useProject((s) => ideMode(s.settings));
  const hidden = useProject((s) => !s.project || s.lightEdit);
  if (hidden) return null;
  const light = mode === "light";
  return (
    <BarButton
      variant="title"
      className={styles.button}
      aria-pressed={!light}
      aria-label={light ? tx("IDE mode: Light") : tx("IDE mode: Full")}
      tooltip={
        light
          ? tx("Light mode: editor, git and search only, no language servers. Click to switch to Full mode.")
          : tx("Full mode: language servers, run and debug are on. Click to switch to Light mode.")
      }
      shortcut={shortcutLabel("ide.toggleMode")}
      onClick={() => void executeAction("ide.toggleMode", "widget")}
    >
      <Icon name={light ? "ide-mode-light" : "ide-mode-full"} size={16} />
      {light ? tx("Light mode") : tx("Full mode")}
    </BarButton>
  );
}
