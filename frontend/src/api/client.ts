// 后端 API 统一响应类型
export interface ApiResponse<T> {
  state: "success" | "failed"
  data: T | null
  error: { code: string; message: string } | null
  meta: { project: string }
}

export interface ProjectInfo {
  project_id: string
  display_name: string
  db_scope: string
  is_share: boolean
}

export interface MemoryItem {
  memory_uuid: string
  revision: number
  category: string
  title_norm: string
  summary: string | null
  content?: string | null
  recall_when?: string | null
  status: string
  keywords?: string[]
  has_open_change: boolean
  change_action: string | null
  created_at: string
  updated_at: string
}

export interface MemoryUpdateInput {
  expected_revision: number
  title_norm: string
  summary: string | null
  recall_when: string | null
  content: string
  keywords: string[]
}

export interface ChangeItem {
  memory_uuid: string
  action: string
  title_norm: string
  summary: string | null
  created_at: string
  updated_at: string
}

export interface ChangeDetail {
  memory_uuid: string
  action: string
  title_norm: string
  summary: string | null
  before_state: Record<string, unknown> | null
  after_state: Record<string, unknown> | null
  created_at: string
  updated_at: string
}

export interface TrashItem extends ChangeItem {
  trashed_at: string
  expires_at: string
}

export interface TrashDetail extends ChangeDetail {
  trashed_at: string
  expires_at: string
}

// Graph API 类型
export interface GraphStatus {
  graph_name: string
  dirty: boolean
  updated_at: string
  memory_count: number
  relation_count: number
}

export interface AuthTokenStatus {
  valid: boolean
  expires_at: number | null
  turnstile_site_key: string | null
}

export interface AuthRefreshResult {
  auth_token: string
  expires_at: number
}

export interface NeighborMemory {
  memory_uuid: string
  category: string
  title_norm: string
  summary: string | null
  status: string
}

export interface NeighborRelation {
  relation_uuid: string
  direction: "incoming" | "outgoing"
  relation_type: string
  weight: number | null
  note: string | null
  memory: NeighborMemory
}

export interface NeighborsResponse {
  memory_uuid: string
  memory: NeighborMemory
  neighbors: NeighborRelation[]
}

export interface GraphOverviewRelation {
  relation_uuid: string
  from_memory_uuid: string
  to_memory_uuid: string
  relation_type: string
  weight: number | null
  note: string | null
}

export interface GraphOverview {
  nodes: NeighborMemory[]
  relations: GraphOverviewRelation[]
}

export interface SuggestedRelation {
  from_memory_uuid: string
  to_memory_uuid: string
  relation_type: string
  weight: number | null
  note: string | null
  candidate: {
    memory_uuid: string
    category: string
    title_norm: string
    summary: string | null
    shared_keywords: number
  }
}

export const RELATION_TYPES = [
  "related_to",
  "supersedes",
  "depends_on",
  "conflicts_with",
  "elaborates",
  "applies_to",
] as const

export type RelationType = (typeof RELATION_TYPES)[number]

export class ApiError extends Error {
  code: string
  constructor(code: string, message: string) {
    super(message)
    this.code = code
    this.name = "ApiError"
  }
}

let projectHeader = ""
// Why：生产环境只配置一条 /api 反代，避免 API 路径和静态资源争抢根路径。
const API_PREFIX = "/api"

export function setProjectHeader(project: string) {
  projectHeader = project
}

async function request<T>(url: string, options?: RequestInit): Promise<T> {
  const headers: Record<string, string> = { ...((options?.headers as Record<string, string>) || {}) }
  if (projectHeader) headers["X-Mem-Project"] = projectHeader
  const res = await fetch(`${API_PREFIX}${url}`, { ...options, headers, credentials: "include" })
  const body: ApiResponse<T> = await res.json()
  if (body.state === "failed") throw new ApiError(body.error!.code, body.error!.message)
  return body.data as T
}

export const api = {
  auth: {
    verify: (key: string) =>
      request<void>("/auth/verify", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ key }),
      }),
    session: () => request<void>("/auth/session"),
    status: () => request<AuthTokenStatus>("/auth/status"),
    refresh: (turnstileToken: string) =>
      request<AuthRefreshResult>("/auth/refresh", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ turnstile_token: turnstileToken }),
      }),
    forceRefresh: () =>
      request<AuthRefreshResult>("/auth/refresh/force", {
        method: "POST",
      }),
  },
  projects: {
    list: () => request<ProjectInfo[]>("/projects"),
  },
  memories: {
    list: () => request<MemoryItem[]>("/memories"),
    categoryKeywords: (category: string) => request<string[]>(`/memories/categories/${encodeURIComponent(category)}/keywords`),
    update: (uuid: string, body: MemoryUpdateInput) =>
      request<void>(`/memories/${encodeURIComponent(uuid)}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      }),
  },
  changes: {
    list: () => request<ChangeItem[]>("/changes"),
    detail: (uuid: string) => request<ChangeDetail>(`/changes/${uuid}`),
    approve: (uuid: string) => request<void>(`/changes/${uuid}/approve`, { method: "POST" }),
    reject: (uuid: string) => request<void>(`/changes/${uuid}/reject`, { method: "POST" }),
  },
  trash: {
    list: () => request<TrashItem[]>("/trash"),
    detail: (uuid: string) => request<TrashDetail>(`/trash/${uuid}`),
    delete: (uuid: string) => request<void>(`/trash/${uuid}/delete`, { method: "POST" }),
    restore: (uuid: string) => request<void>(`/trash/${uuid}/restore`, { method: "POST" }),
  },
  graph: {
    status: () => request<GraphStatus>("/graph/status"),
    overview: () => request<GraphOverview>("/graph/overview"),
    rebuild: () => request<{ graph: string }>("/graph/rebuild", { method: "POST" }),
    neighbors: (uuid: string) => request<NeighborsResponse>(`/graph/neighbors/${uuid}`),
    suggest: (uuid: string) => request<SuggestedRelation[]>(`/graph/relations/suggest/${uuid}`),
    addRelation: (body: { from_memory_uuid: string; to_memory_uuid: string; relation_type: string; weight?: number; note?: string }) =>
      request<Record<string, unknown>>("/graph/relations", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      }),
    updateRelation: (uuid: string, body: { relation_type?: string; weight?: number; note?: string }) =>
      request<Record<string, unknown>>(`/graph/relations/${uuid}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      }),
    deleteRelation: (uuid: string) =>
      request<{ deleted: boolean }>(`/graph/relations/${uuid}`, { method: "DELETE" }),
  },
}
