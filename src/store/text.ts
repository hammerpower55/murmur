// Dictionary, snippets and the Roman Urdu table. Rust saves and validates;
// this keeps the last saved copy for the pages.

import { create } from "zustand";
import { api, type DictEntry, type Snippet, type TextData, type UrduRow } from "../lib/api";

interface TextState {
  data: TextData | null;
  load: () => Promise<void>;
  setDictionary: (entries: DictEntry[]) => Promise<void>;
  setSnippets: (snippets: Snippet[]) => Promise<void>;
  setRomanUrdu: (rows: UrduRow[]) => Promise<void>;
  resetRomanUrdu: () => Promise<void>;
}

export const useText = create<TextState>((set, get) => {
  const patch = (p: Partial<TextData>) => {
    const data = get().data;
    if (data) set({ data: { ...data, ...p } });
  };
  return {
    data: null,
    load: async () => set({ data: await api.textData() }),
    setDictionary: async (dictionary) => {
      await api.setDictionary(dictionary);
      patch({ dictionary });
    },
    setSnippets: async (snippets) => {
      await api.setSnippets(snippets);
      patch({ snippets });
    },
    setRomanUrdu: async (romanUrdu) => {
      await api.setRomanUrdu(romanUrdu);
      patch({ romanUrdu });
    },
    resetRomanUrdu: async () => patch({ romanUrdu: await api.resetRomanUrdu() }),
  };
});
