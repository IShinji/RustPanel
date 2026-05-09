import { createClient } from "@connectrpc/connect";
import { createConnectTransport } from "@connectrpc/connect-web";

import { SystemService } from "../gen/rustpanel/v1/system_pb";

export function createSystemClient(baseUrl: string) {
  const transport = createConnectTransport({ baseUrl });

  return createClient(SystemService, transport);
}
