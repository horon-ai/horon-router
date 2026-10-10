import { createFileRoute } from "@tanstack/react-router";
import { Credentials } from "../pages/Credentials";
export const Route = createFileRoute("/credentials/$id")({
  component: () => <Credentials credentialId={Route.useParams().id} />,
});
