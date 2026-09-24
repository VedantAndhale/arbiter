import { createContext, useContext } from "react";
import type { Api } from "./api";

// Keep provider identity outside the App/component import cycle during hot reload.
export const ApiContext = createContext<Api | null>(null);
export function useApi(): Api {
  const api = useContext(ApiContext);
  if (!api) throw new Error("useApi outside ApiContext");
  return api;
}
