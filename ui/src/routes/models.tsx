import { createFileRoute } from "@tanstack/react-router";
import { Models } from "../pages/Models";
export const Route = createFileRoute("/models")({ component: Models });
