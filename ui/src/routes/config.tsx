import { createFileRoute } from "@tanstack/react-router";
import { Config } from "../pages/Config";
export const Route = createFileRoute("/config")({ component: Config });
