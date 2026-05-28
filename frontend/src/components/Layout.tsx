import { Outlet, Link, useNavigate, useLocation } from "react-router-dom"
import { useAuth } from "@/auth/AuthContext"
import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"
import { Input } from "@/components/ui/input"
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { FileText, Clock, LogOut, Menu, X, Search, Monitor, Moon, Sun, ChevronDown, GitBranch, ShieldCheck } from "lucide-react"
import { useState, useEffect } from "react"
import { cn } from "@/lib/utils"
import type { ProjectInfo } from "@/api/client"

const navItems = [
  { to: "/memories", icon: FileText, label: "记忆" },
  { to: "/changes", icon: Clock, label: "待确认" },
  { to: "/graph", icon: GitBranch, label: "图谱" },
  { to: "/auth", icon: ShieldCheck, label: "授权" },
]

type Theme = "system" | "light" | "dark"

function getSystemTheme(): "light" | "dark" {
  if (typeof window === "undefined") return "dark"
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"
}

const THEME_KEY = "mem_theme"

export function Layout() {
  const { activeProject, projects, selectProject, logout } = useAuth()
  const navigate = useNavigate()
  const location = useLocation()
  const [sidebarOpen, setSidebarOpen] = useState(false)
  const [projectOpen, setProjectOpen] = useState(false)
  const [theme, setTheme] = useState<Theme>(() => {
    if (typeof window === "undefined") return "system"
    return (localStorage.getItem(THEME_KEY) as Theme) || "system"
  })

  // 同步主题到 <html>
  useEffect(() => {
    const root = document.documentElement
    const applied = theme === "system" ? getSystemTheme() : theme
    root.classList.toggle("dark", applied === "dark")
    localStorage.setItem(THEME_KEY, theme)
  }, [theme])

  // 监听系统主题变化
  useEffect(() => {
    if (theme !== "system") return
    const mq = window.matchMedia("(prefers-color-scheme: dark)")
    const handler = () => {
      document.documentElement.classList.toggle("dark", mq.matches)
    }
    mq.addEventListener("change", handler)
    return () => mq.removeEventListener("change", handler)
  }, [theme])

  const handleLogout = () => {
    logout()
    navigate("/login")
  }

  const pageTitle = location.pathname === "/memories" ? "Projects" : location.pathname === "/changes" ? "待确认" : location.pathname === "/graph" ? "图谱" : location.pathname === "/auth" ? "授权" : "Mem"
  // 当前主题图标组件
  const ThemeIcon = theme === "system" ? Monitor : theme === "dark" ? Moon : Sun

  return (
    <div className="min-h-screen bg-background flex">
      {/* 左侧栏 */}
      <aside className="hidden sm:flex flex-col w-56 border-r bg-card shrink-0">
        {/* Logo + Workspace */}
        <div className="px-4 py-3 border-b space-y-3">
          <Link to="/memories" className="text-sm font-bold text-foreground hover:text-foreground/80 transition-colors">Mem</Link>
          <DropdownMenu open={projectOpen} onOpenChange={setProjectOpen}>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="sm" className="gap-1 text-sm font-medium w-full justify-start px-2 -ml-2">
                {activeProject?.display_name || "选择项目"}
                <ChevronDown className="h-3 w-3 text-muted-foreground ml-auto" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-48">
              {projects.map((p: ProjectInfo) => (
                <DropdownMenuItem key={p.project_id} onClick={() => selectProject(p)} className="text-sm">
                  {p.display_name}
                  {activeProject?.project_id === p.project_id && <span className="ml-auto text-muted-foreground text-xs">当前</span>}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        <nav className="flex-1 px-2 py-3 space-y-1">
          {navItems.map((item) => {
            const active = location.pathname === item.to
            return (
              <Link key={item.to} to={item.to} className={cn(
                "flex items-center gap-2 px-3 py-2 rounded-md text-sm transition-colors",
                active ? "bg-accent text-foreground font-medium" : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
              )}>
                <item.icon className="h-4 w-4" />
                {item.label}
              </Link>
            )
          })}
        </nav>
        <div className="px-2 py-3 border-t space-y-1">
          <button onClick={handleLogout} className="flex items-center gap-2 px-3 py-2 rounded-md text-sm text-muted-foreground hover:text-foreground hover:bg-accent/50 w-full text-left transition-colors">
            <LogOut className="h-4 w-4" />退出
          </button>
        </div>
      </aside>

      {/* 移动端侧栏 */}
      {sidebarOpen && <div className="fixed inset-0 z-40 bg-black/50 sm:hidden" onClick={() => setSidebarOpen(false)} />}
      <aside className={cn(
        "fixed inset-y-0 left-0 z-50 w-56 bg-card border-r flex flex-col sm:hidden transition-transform duration-200",
        sidebarOpen ? "translate-x-0" : "-translate-x-full"
      )}>
        <div className="h-12 flex items-center justify-between px-4 border-b">
          <span className="text-sm font-semibold text-foreground truncate">{activeProject?.display_name || "Mem"}</span>
          <Button variant="ghost" size="icon" onClick={() => setSidebarOpen(false)}><X className="h-4 w-4" /></Button>
        </div>
        <nav className="flex-1 px-2 py-3 space-y-1">
          {navItems.map((item) => {
            const active = location.pathname === item.to
            return (
              <Link key={item.to} to={item.to} onClick={() => setSidebarOpen(false)} className={cn(
                "flex items-center gap-2 px-3 py-2 rounded-md text-sm transition-colors",
                active ? "bg-accent text-foreground font-medium" : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
              )}>
                <item.icon className="h-4 w-4" />{item.label}
              </Link>
            )
          })}
        </nav>
        <div className="px-2 py-3 border-t space-y-1">
          <button onClick={handleLogout} className="flex items-center gap-2 px-3 py-2 rounded-md text-sm text-muted-foreground hover:text-foreground hover:bg-accent/50 w-full text-left transition-colors">
            <LogOut className="h-4 w-4" />退出
          </button>
        </div>
      </aside>

      {/* 右侧：顶栏 + 内容 */}
      <div className="flex-1 min-w-0 flex flex-col">
        {/* 顶栏 */}
        <header className="h-12 flex items-center px-4 border-b bg-card shrink-0 gap-3">
          {/* 移动汉堡 */}
          <Button variant="ghost" size="icon" className="sm:hidden" onClick={() => setSidebarOpen(true)}>
            <Menu className="h-4 w-4" />
          </Button>

          {/* 页面标题 */}
          <span className="text-sm font-semibold text-foreground hidden sm:block">{pageTitle}</span>

          <div className="flex-1" />

          {/* 搜索 */}
          <div className="hidden sm:flex relative max-w-xs">
            <Search className="absolute left-2 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground" />
            <Input className="h-7 pl-7 text-xs bg-muted/50 border-transparent focus:border-border" placeholder="搜索..." />
          </div>

          {/* 主题切换 */}
          <Separator orientation="vertical" className="h-4 hidden sm:block" />
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" className="h-7 w-7 ml-auto sm:ml-0">
                <ThemeIcon className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-36">
              <DropdownMenuItem onClick={() => setTheme("light")} className="text-sm gap-2">
                <Sun className="h-4 w-4" />浅色
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => setTheme("dark")} className="text-sm gap-2">
                <Moon className="h-4 w-4" />深色
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => setTheme("system")} className="text-sm gap-2">
                <Monitor className="h-4 w-4" />系统
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </header>

        <main className="flex-1 relative">
          <Outlet />
        </main>
      </div>
    </div>
  )
}
