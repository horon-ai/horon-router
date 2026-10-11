import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createHashHistory, createRouter } from "@tanstack/react-router";
import { routeTree } from "./routeTree.gen";
import { queryClient } from "./queries";

const history = createHashHistory();
// Keep the panel's fragment-only link URLs while retaining indexed router navigation.
history.createHref = (href) => `#${href}`;
export const router = createRouter({ routeTree, history });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

export function AppRouter() {
  return (
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}
