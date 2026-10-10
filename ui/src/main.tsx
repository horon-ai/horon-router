import { createRoot } from "react-dom/client";
import { AppRouter } from "./router";
import "./style.css";

if (/^#[^/]/.test(location.hash)) location.hash = `#/${location.hash.slice(1)}`;

const app = document.getElementById("app");
if (!app) throw new Error("Missing #app");
createRoot(app).render(<AppRouter />);
