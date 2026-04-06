import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import { Toaster } from "sonner";
import { AppLayout } from "@/components/layout/AppLayout";
import { AdminLayout } from "@/components/layout/AdminLayout";
import { ProtectedRoute } from "@/components/ProtectedRoute";
import LoginPage from "@/pages/LoginPage";
import SignupPage from "@/pages/SignupPage";
import DashboardPage from "@/pages/DashboardPage";
import ChatPage from "@/pages/ChatPage";
import UsagePage from "@/pages/UsagePage";
import SchedulesPage from "@/pages/SchedulesPage";
import NotificationsPage from "@/pages/NotificationsPage";
import AdminUsersPage from "@/pages/admin/AdminUsersPage";
import AdminAgentsPage from "@/pages/admin/AdminAgentsPage";
import AdminAnalyticsPage from "@/pages/admin/AdminAnalyticsPage";
import AdminHealthPage from "@/pages/admin/AdminHealthPage";

export default function App() {
  return (
    <BrowserRouter>
      <Toaster position="top-right" richColors closeButton />
      <Routes>
        {/* Public routes */}
        <Route path="/login" element={<LoginPage />} />
        <Route path="/signup" element={<SignupPage />} />

        {/* Protected routes */}
        <Route
          element={
            <ProtectedRoute>
              <AppLayout />
            </ProtectedRoute>
          }
        >
          <Route path="/dashboard" element={<DashboardPage />} />
          <Route path="/chat" element={<ChatPage />} />
          <Route path="/chat/:id" element={<ChatPage />} />
          <Route path="/usage" element={<UsagePage />} />
          <Route path="/schedules" element={<SchedulesPage />} />
          <Route path="/notifications" element={<NotificationsPage />} />
        </Route>

        {/* Admin routes */}
        <Route
          element={
            <ProtectedRoute>
              <AdminLayout />
            </ProtectedRoute>
          }
        >
          <Route path="/admin" element={<Navigate to="/admin/users" replace />} />
          <Route path="/admin/users" element={<AdminUsersPage />} />
          <Route path="/admin/agents" element={<AdminAgentsPage />} />
          <Route path="/admin/analytics" element={<AdminAnalyticsPage />} />
          <Route path="/admin/health" element={<AdminHealthPage />} />
        </Route>

        {/* Fallback */}
        <Route path="*" element={<Navigate to="/dashboard" replace />} />
      </Routes>
    </BrowserRouter>
  );
}
