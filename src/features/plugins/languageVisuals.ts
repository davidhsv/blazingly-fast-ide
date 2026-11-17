/** Embedded highlighting and command gutter actions share the editor-provider refresh. */
import { Prec, RangeSet, StateEffect, StateField, type Extension, type EditorState } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  GutterMarker,
  ViewPlugin,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";
import { highlightingFor } from "@codemirror/language";
import { highlightTree } from "@lezer/highlight";
import { iconAreaSource, mapLineStart, shownAgain, type EditorDocContext } from "@/editor/api";
import { rangeToOffsets } from "@/editor/lsp/positions";
import { pluginVisuals } from "@/editor/lsp/pluginVisuals";
import { findLanguageDescription, languageInfo, loadSupport } from "@/editor/core/cm/languages";
import { onNavigationProvidersChanged } from "@/editor/navigationProviders";
import { call, commands, type GutterAction, type Injection } from "@/platform/ipc";
import { gutterIconMarkup, isIconName, type IconName } from "@/ui/icons";
import { showContextMenu } from "@/ui";
import { resolveMarkerIcon, onMarkerIconsChanged } from "./markerIcons";
import { markerIcon } from "./lineMarkerGutter";
import styles from "@/editor/navigation/LineMarkers.module.css";

export async function injectedHighlights(state: EditorState, injections: readonly Injection[]): Promise<DecorationSet> {
  const ranges = await Promise.all(
    injections.map(async (i) => {
      const { from, to } = rangeToOffsets(state.doc, i.range);
      if (from >= to) return [];
      const desc = findLanguageDescription(languageInfo(i.language)?.uiMode ?? i.language, "");
      const support = desc ? await loadSupport(desc) : null;
      if (!support) return [];
      const marks = [] as ReturnType<Decoration["range"]>[];
      highlightTree(
        support.language.parser.parse(state.sliceDoc(from, to)),
        { style: (tags) => highlightingFor(state, tags) },
        (a, b, cls) => marks.push(Decoration.mark({ class: cls }).range(from + a, from + b)),
      );
      return marks;
    }),
  );
  return Decoration.set(ranges.flat(), true);
}
const setHighlight = StateEffect.define<DecorationSet>();
const highlights = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setHighlight)) return e.value;
    return tr.docChanged ? Decoration.none : value;
  },
  provide: (f) => Prec.high(EditorView.decorations.from(f)),
});
interface LineAction {
  plugin: string;
  action: GutterAction;
}
/** The actions of one line: a click runs a single one; several (Run, Debug…) open a menu, as IDEA's run gutter does. */
class ActionMarker extends GutterMarker {
  constructor(
    readonly actions: readonly LineAction[],
    readonly ctx: EditorDocContext,
  ) {
    super();
  }
  override eq(other: GutterMarker) {
    // The line is left out: a marker whose line moved keeps its button mounted.
    return (
      other instanceof ActionMarker &&
      other.actions.length === this.actions.length &&
      other.actions.every(({ plugin, action }, i) => {
        const mine = this.actions[i]!;
        return (
          plugin === mine.plugin &&
          action.icon === mine.action.icon &&
          action.tooltip === mine.action.tooltip &&
          action.command === mine.action.command &&
          action.argument === mine.action.argument
        );
      })
    );
  }
  private execute({ plugin, action }: LineAction) {
    void call(
      commands.lspExecuteCommand(this.ctx.doc, {
        title: action.tooltip,
        command: `openidea.plugin-language:${plugin}:${action.command}`,
        arguments: [JSON.stringify(action.argument)],
      }),
    ).catch(console.warn);
  }
  override toDOM() {
    const first = this.actions[0]!;
    const el = document.createElement("button");
    el.type = "button";
    el.className = `${styles.marker} cm-plugin-gutter-action`;
    const title = this.actions.map((a) => a.action.tooltip).join("\n");
    el.title = title;
    el.setAttribute("aria-label", title);
    el.innerHTML = gutterIconMarkup(markerIcon(first.action.icon)) ?? "";
    el.onclick = (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (this.actions.length === 1) {
        this.execute(first);
        return;
      }
      const at = el.getBoundingClientRect();
      showContextMenu(
        { x: at.left, y: at.bottom },
        this.actions.map((a) => ({
          label: a.action.tooltip,
          icon: menuIcon(a.action.icon),
          run: () => this.execute(a),
        })),
      );
    };
    return el;
  }
}
/** The menu icon of an action whose gutter icon is a registered icon name (`run`, `debug`). */
function menuIcon(icon: string | null): IconName | undefined {
  return icon && isIconName(icon) ? icon : undefined;
}
const setActions = StateEffect.define<{ pos: number; marker: ActionMarker }[]>();
const actions = StateField.define<{ pos: number; marker: ActionMarker }[]>({
  create: () => [],
  update(v, tr) {
    for (const e of tr.effects) if (e.is(setActions)) return e.value;
    if (!tr.docChanged) return v;
    return v.map((a) => ({ ...a, pos: mapLineStart(tr, a.pos) }));
  },
});
/** In the shared icon area, beside the plugins' line markers. */
const actionSource = iconAreaSource.of({
  order: 65,
  markers: (view) =>
    RangeSet.of(
      view.state.field(actions).map((a) => a.marker.range(a.pos)),
      true,
    ),
});
export function pluginLanguageVisuals(ctx: EditorDocContext): Extension {
  if (ctx.doc < 0) return [];
  return [
    highlights,
    actions,
    actionSource,
    ViewPlugin.define(
      (view) =>
        new (class {
          timer: ReturnType<typeof setTimeout> | undefined;
          generation = 0;
          skipped = false;
          off: () => void;
          constructor() {
            const a = onNavigationProvidersChanged(() => this.schedule());
            const b = onMarkerIconsChanged(() => this.schedule());
            this.off = () => {
              a();
              b();
            };
            this.schedule();
          }
          schedule() {
            this.generation++;
            clearTimeout(this.timer);
            this.timer = setTimeout(() => void this.run(), 300);
          }
          update(u: ViewUpdate) {
            if (u.docChanged || (this.skipped && shownAgain(u))) this.schedule();
          }
          async run() {
            const generation = this.generation;
            const text = view.state.doc;
            try {
              const result = await pluginVisuals(view, ctx);
              const deco = await injectedHighlights(view.state, result.injections);
              const resolved = await Promise.all(
                result.gutterActions
                  .filter(([, a]) => a.line < text.lines)
                  .map(async ([plugin, a]) => ({ plugin, action: a, icon: await resolveMarkerIcon(plugin, a.icon) })),
              );
              const byLine = new Map<number, LineAction[]>();
              for (const { plugin, action, icon } of resolved) {
                const line = byLine.get(action.line) ?? [];
                line.push({ plugin, action: { ...action, icon } });
                byLine.set(action.line, line);
              }
              const placed = [...byLine]
                .sort(([a], [b]) => a - b)
                .map(([line, entries]) => ({ pos: text.line(line + 1).from, marker: new ActionMarker(entries, ctx) }));
              if (generation !== this.generation || view.state.doc !== text) return;
              this.skipped = !view.dom.isConnected;
              if (this.skipped) return;
              view.dispatch({ effects: [setHighlight.of(deco), setActions.of(placed)] });
            } catch (error) {
              console.debug("[plugin language visuals]", error);
            }
          }
          destroy() {
            this.generation++;
            clearTimeout(this.timer);
            this.off();
          }
        })(),
    ),
  ];
}
