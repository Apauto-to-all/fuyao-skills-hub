import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { Toaster } from "@/components/ui/toast";
import { Shell } from "./app/shell";

function App() {
  // 每次挂载一份独立缓存：查询失败不自动重试，重试交给页面上的按钮
  const [client] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: { retry: false, refetchOnWindowFocus: false },
        },
      }),
  );

  return (
    <QueryClientProvider client={client}>
      <Toaster>
        <Shell />
      </Toaster>
    </QueryClientProvider>
  );
}

export default App;
