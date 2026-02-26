import { BrowserRouter, Routes, Route } from "react-router-dom";
import Home from "./pages/Home";
import BlockList from "./pages/BlockList";
import BlockDetail from "./pages/BlockDetail";

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<Home />} />
        <Route path="/blocks" element={<BlockList />} />
        <Route path="/block/:number" element={<BlockDetail />} />
      </Routes>
    </BrowserRouter>
  );
}
