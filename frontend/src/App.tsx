import { Routes, Route, Navigate } from "react-router-dom"
import { AuthProvider } from "@/auth/AuthProvider"
import { ProtectedRoute } from "@/auth/ProtectedRoute"
import { ProjectGuard } from "@/auth/ProjectGuard"
import { Layout } from "@/components/Layout"
import { LoginPage } from "@/pages/LoginPage"
import { ProjectsPage } from "@/pages/ProjectsPage"
import { MemoriesPage } from "@/pages/MemoriesPage"
import { ChangesPage } from "@/pages/ChangesPage"

export default function App() {
  return (
    <AuthProvider>
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route element={<ProtectedRoute />}>
          <Route path="/projects" element={<ProjectsPage />} />
          <Route element={<ProjectGuard />}>
            <Route element={<Layout />}>
              <Route path="/memories" element={<MemoriesPage />} />
              <Route path="/changes" element={<ChangesPage />} />
            </Route>
          </Route>
          <Route path="/" element={<Navigate to="/memories" replace />} />
          <Route path="*" element={<Navigate to="/memories" replace />} />
        </Route>
      </Routes>
    </AuthProvider>
  )
}