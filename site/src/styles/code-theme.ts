// Shiki theme built from the Oxide tokens (see tokens.css), so code
// samples use the same two hues as the rest of the site: rust for
// keywords and the things you type, patina for strings and values.

export const oxideTheme = {
  name: "oxide",
  type: "dark" as const,
  colors: {
    "editor.background": "#07090a",
    "editor.foreground": "#c9d0d6",
  },
  tokenColors: [
    { scope: ["comment", "punctuation.definition.comment"], settings: { foreground: "#5a6570", fontStyle: "italic" } },
    { scope: ["keyword", "storage", "storage.type", "keyword.operator.new"], settings: { foreground: "#ff8a52" } },
    { scope: ["entity.name.function", "support.function", "entity.name.command"], settings: { foreground: "#eceff1" } },
    { scope: ["string", "string.quoted", "markup.inline.raw"], settings: { foreground: "#8fe3d8" } },
    { scope: ["constant.numeric", "constant.language", "constant.other.symbol"], settings: { foreground: "#5fcfc1" } },
    { scope: ["entity.name.type", "support.type", "entity.name.namespace", "support.class"], settings: { foreground: "#ffb088" } },
    { scope: ["variable.parameter", "variable.other.readwrite", "variable"], settings: { foreground: "#c9d0d6" } },
    { scope: ["punctuation", "keyword.operator"], settings: { foreground: "#7d8994" } },
  ],
};
