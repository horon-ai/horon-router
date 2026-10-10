import { createFileRoute, redirect } from "@tanstack/react-router";
export const Route = createFileRoute("/oauth")({ beforeLoad: () => { throw redirect({ to: "/connect" }); } });
