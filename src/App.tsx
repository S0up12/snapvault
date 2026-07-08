import { HashRouter, Route, Routes } from "react-router-dom";

import Layout from "./components/Layout";
import Chats from "./views/Chats";
import Dashboard from "./views/Dashboard";
import Memories from "./views/Memories";
import Profile from "./views/Profile";
import Settings from "./views/Settings";
import Stories from "./views/Stories";

export default function App() {
  return (
    <HashRouter>
      <Routes>
        <Route element={<Layout />}>
          <Route index element={<Dashboard />} />
          <Route path="/profile" element={<Profile />} />
          <Route path="/chats" element={<Chats />} />
          <Route path="/memories" element={<Memories />} />
          <Route path="/stories" element={<Stories />} />
          <Route path="/settings" element={<Settings />} />
        </Route>
      </Routes>
    </HashRouter>
  );
}
