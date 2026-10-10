import { createFileRoute, redirect } from "@tanstack/react-router";
export const Route = createFileRoute("/use")({ beforeLoad: () => { throw redirect({ to: "/keys" }); } });
