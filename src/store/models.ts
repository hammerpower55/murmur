// Models page state. Rust is the source of truth: the store refetches the
// whole view on `models-changed` and patches progress from `model-download`.

import { create } from "zustand";
import { api, errorText, events, type ModelsView } from "../lib/api";

interface ModelsState {
  view: ModelsView | null;
  /** One line per model id, from failed downloads, loads or deletes. */
  errors: Record<string, string>;
  refresh: () => Promise<void>;
  download: (id: string) => Promise<void>;
  cancel: (id: string) => Promise<void>;
  use: (id: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  dismissError: (id: string) => void;
}

export const useModels = create<ModelsState>((set, get) => {
  const setError = (id: string, message: string) =>
    set((s) => ({ errors: { ...s.errors, [id]: message } }));
  const run = async (id: string, action: () => Promise<unknown>) => {
    get().dismissError(id);
    try {
      await action();
    } catch (err) {
      setError(id, errorText(err));
    }
    await get().refresh();
  };

  return {
    view: null,
    errors: {},
    refresh: async () => set({ view: await api.listModels() }),
    download: (id) => run(id, () => api.downloadModel(id)),
    cancel: (id) => run(id, () => api.cancelDownload(id)),
    use: (id) => run(id, () => api.useModel(id)),
    remove: (id) => run(id, () => api.deleteModel(id)),
    dismissError: (id) =>
      set((s) => {
        const errors = { ...s.errors };
        delete errors[id];
        return { errors };
      }),
  };
});

let subscribed = false;

/** Starts listening once for the life of the window. */
export function subscribeModels() {
  if (subscribed) return;
  subscribed = true;
  const { refresh } = useModels.getState();
  void refresh();
  void events.modelsChanged(() => void refresh());
  void events.modelDownload(({ id, done, total }) =>
    useModels.setState((s) =>
      s.view ? { view: { ...s.view, downloads: { ...s.view.downloads, [id]: { done, total } } } } : s,
    ),
  );
  void events.modelError(({ id, message }) =>
    useModels.setState((s) => ({ errors: { ...s.errors, [id]: message } })),
  );
}
