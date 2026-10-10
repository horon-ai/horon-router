import { createFileRoute } from "@tanstack/react-router";
import { Quotas } from "../pages/Quotas";
export const Route = createFileRoute("/quotas")({ component: Quotas });
