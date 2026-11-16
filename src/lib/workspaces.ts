export const workspaces = ["chat", "models", "speech", "embeddings", "settings"] as const;

export type Workspace = (typeof workspaces)[number];
export type PrimaryWorkspace = Exclude<Workspace, "settings">;
