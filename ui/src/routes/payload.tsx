import { createFileRoute } from "@tanstack/react-router";
import { Payload } from "../pages/Payload";
export const Route = createFileRoute("/payload")({ component: Payload });
