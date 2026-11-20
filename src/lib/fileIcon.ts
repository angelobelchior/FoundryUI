import {
  BookIcon,
  CodeIcon,
  ContainerIcon,
  FileIcon,
  FolderIcon,
  GearIcon,
  GitBranchIcon,
  ImageIcon,
  JsonIcon,
  LockIcon,
  PackageIcon,
  TerminalIcon,
  type IconComponent,
} from "../components/Icon";

type EntryKind = "file" | "directory" | "symlink" | "other";

export type FileIconStyle = {
  icon: IconComponent;
  color: string | null;
};

const folderStyles: Record<string, FileIconStyle> = {
  ".git": { icon: GitBranchIcon, color: "text-orange-500" },
  ".github": { icon: GitBranchIcon, color: "text-orange-500" },
  node_modules: { icon: PackageIcon, color: "text-rose-500" },
  dist: { icon: PackageIcon, color: "text-amber-400" },
  build: { icon: PackageIcon, color: "text-amber-400" },
  target: { icon: PackageIcon, color: "text-amber-400" },
  out: { icon: PackageIcon, color: "text-amber-400" },
  ".vscode": { icon: GearIcon, color: "text-sky-500" },
  ".idea": { icon: GearIcon, color: "text-sky-500" },
  src: { icon: CodeIcon, color: null },
  lib: { icon: CodeIcon, color: null },
  test: { icon: CodeIcon, color: null },
  tests: { icon: CodeIcon, color: null },
};

const fileStyles: Record<string, FileIconStyle> = {
  "package.json": { icon: PackageIcon, color: "text-rose-500" },
  "package-lock.json": { icon: PackageIcon, color: "text-rose-500" },
  "yarn.lock": { icon: PackageIcon, color: "text-rose-500" },
  "pnpm-lock.yaml": { icon: PackageIcon, color: "text-rose-500" },
  "pnpm-lock.yml": { icon: PackageIcon, color: "text-rose-500" },
  "tsconfig.json": { icon: GearIcon, color: "text-sky-500" },
  "cargo.toml": { icon: GearIcon, color: "text-orange-500" },
  "cargo.lock": { icon: GearIcon, color: "text-orange-500" },
  "dockerfile": { icon: ContainerIcon, color: "text-sky-400" },
  "docker-compose.yml": { icon: ContainerIcon, color: "text-sky-400" },
  "docker-compose.yaml": { icon: ContainerIcon, color: "text-sky-400" },
  "compose.yml": { icon: ContainerIcon, color: "text-sky-400" },
  "compose.yaml": { icon: ContainerIcon, color: "text-sky-400" },
  ".gitignore": { icon: GitBranchIcon, color: "text-orange-500" },
  ".gitattributes": { icon: GitBranchIcon, color: "text-orange-500" },
  ".gitmodules": { icon: GitBranchIcon, color: "text-orange-500" },
  "makefile": { icon: GearIcon, color: "text-amber-400" },
  "gemfile": { icon: PackageIcon, color: "text-rose-500" },
  "rakefile": { icon: GearIcon, color: "text-amber-400" },
};

const extensionStyles: Record<string, FileIconStyle> = {
  md: { icon: BookIcon, color: "text-sky-500" },
  mdx: { icon: BookIcon, color: "text-sky-500" },
  txt: { icon: BookIcon, color: "text-slate-400" },
  rst: { icon: BookIcon, color: "text-slate-400" },
  json: { icon: JsonIcon, color: "text-amber-400" },
  jsonc: { icon: JsonIcon, color: "text-amber-400" },
  yml: { icon: GearIcon, color: "text-amber-400" },
  yaml: { icon: GearIcon, color: "text-amber-400" },
  toml: { icon: GearIcon, color: "text-amber-400" },
  ini: { icon: GearIcon, color: "text-slate-400" },
  conf: { icon: GearIcon, color: "text-slate-400" },
  config: { icon: GearIcon, color: "text-slate-400" },
  sh: { icon: TerminalIcon, color: "text-emerald-500" },
  bash: { icon: TerminalIcon, color: "text-emerald-500" },
  zsh: { icon: TerminalIcon, color: "text-emerald-500" },
  fish: { icon: TerminalIcon, color: "text-emerald-500" },
  ps1: { icon: TerminalIcon, color: "text-emerald-500" },
  png: { icon: ImageIcon, color: "text-violet-500" },
  jpg: { icon: ImageIcon, color: "text-violet-500" },
  jpeg: { icon: ImageIcon, color: "text-violet-500" },
  gif: { icon: ImageIcon, color: "text-violet-500" },
  webp: { icon: ImageIcon, color: "text-violet-500" },
  svg: { icon: ImageIcon, color: "text-violet-500" },
  ico: { icon: ImageIcon, color: "text-violet-500" },
  avif: { icon: ImageIcon, color: "text-violet-500" },
  bmp: { icon: ImageIcon, color: "text-violet-500" },
  lock: { icon: LockIcon, color: "text-emerald-500" },
  key: { icon: LockIcon, color: "text-emerald-500" },
  pem: { icon: LockIcon, color: "text-emerald-500" },
  env: { icon: LockIcon, color: "text-emerald-500" },
  ts: { icon: CodeIcon, color: "text-sky-500" },
  tsx: { icon: CodeIcon, color: "text-sky-500" },
  js: { icon: CodeIcon, color: "text-yellow-500" },
  jsx: { icon: CodeIcon, color: "text-yellow-500" },
  mjs: { icon: CodeIcon, color: "text-yellow-500" },
  cjs: { icon: CodeIcon, color: "text-yellow-500" },
  rs: { icon: CodeIcon, color: "text-orange-500" },
  py: { icon: CodeIcon, color: "text-blue-500" },
  go: { icon: CodeIcon, color: "text-cyan-400" },
  java: { icon: CodeIcon, color: "text-red-500" },
  c: { icon: CodeIcon, color: "text-blue-500" },
  h: { icon: CodeIcon, color: "text-blue-500" },
  cpp: { icon: CodeIcon, color: "text-blue-500" },
  hpp: { icon: CodeIcon, color: "text-blue-500" },
  cs: { icon: CodeIcon, color: "text-violet-500" },
  rb: { icon: CodeIcon, color: "text-red-500" },
  php: { icon: CodeIcon, color: "text-indigo-500" },
  swift: { icon: CodeIcon, color: "text-orange-500" },
  kt: { icon: CodeIcon, color: "text-violet-500" },
  scala: { icon: CodeIcon, color: "text-red-500" },
  dart: { icon: CodeIcon, color: "text-cyan-400" },
  lua: { icon: CodeIcon, color: "text-sky-500" },
  ex: { icon: CodeIcon, color: "text-violet-500" },
  exs: { icon: CodeIcon, color: "text-violet-500" },
  html: { icon: CodeIcon, color: "text-orange-500" },
  htm: { icon: CodeIcon, color: "text-orange-500" },
  css: { icon: CodeIcon, color: "text-blue-500" },
  scss: { icon: CodeIcon, color: "text-pink-500" },
  less: { icon: CodeIcon, color: "text-blue-500" },
  vue: { icon: CodeIcon, color: "text-emerald-500" },
  svelte: { icon: CodeIcon, color: "text-orange-500" },
  astro: { icon: CodeIcon, color: "text-orange-500" },
  sql: { icon: CodeIcon, color: "text-sky-500" },
  graphql: { icon: CodeIcon, color: "text-pink-500" },
  gql: { icon: CodeIcon, color: "text-pink-500" },
};

export function fileIcon(name: string, kind: EntryKind): FileIconStyle {
  const lower = name.toLowerCase();

  if (kind === "directory") {
    return folderStyles[lower] ?? { icon: FolderIcon, color: null };
  }

  if (kind !== "file") {
    return { icon: FileIcon, color: null };
  }

  if (fileStyles[lower]) {
    return fileStyles[lower];
  }

  if (lower.startsWith("readme")) return { icon: BookIcon, color: "text-sky-500" };
  if (lower.startsWith("tsconfig")) return { icon: GearIcon, color: "text-sky-500" };
  if (lower.startsWith("dockerfile")) return { icon: ContainerIcon, color: "text-sky-400" };
  if (lower.startsWith(".env")) return { icon: LockIcon, color: "text-emerald-500" };
  if (lower.startsWith("makefile")) return { icon: GearIcon, color: "text-amber-400" };

  const dot = lower.lastIndexOf(".");
  const extension = dot > 0 ? lower.slice(dot + 1) : "";

  return extensionStyles[extension] ?? { icon: FileIcon, color: null };
}
