import { createFileRoute } from "@tanstack/react-router";
import { Plugins } from "../pages/Plugins";
export const Route = createFileRoute("/plugins")({ component: Plugins });
