import { motion, AnimatePresence } from "framer-motion";
import { Shield, Trash2, AlertTriangle, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";

export interface BlacklistEntry {
  battletag: string;
  memo: string;
}

interface BlacklistPanelProps {
  entries: BlacklistEntry[];
  onRemove: (battletag: string) => void;
  onUpdateMemo: (battletag: string, memo: string) => void;
  onAddEntry: (battletag: string, memo: string) => void;
}

export const BlacklistPanel = ({
  entries,
  onRemove,
  onUpdateMemo,
  onAddEntry,
}: BlacklistPanelProps) => {
  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4, delay: 0.2 }}
      className="card-cyber rounded-lg overflow-hidden h-full flex flex-col"
    >
      {/* Header */}
      <div className="px-5 py-4 border-b border-border/50 flex items-center gap-3">
        <div className="w-8 h-8 rounded-lg bg-destructive/20 border border-destructive/30 flex items-center justify-center">
          <Shield className="h-4 w-4 text-destructive" />
        </div>
        <div>
          <h3 className="font-display text-base font-semibold text-foreground">
            블랙리스트
          </h3>
          <p className="text-xs text-muted-foreground">
            자동 차단 대상 ({entries.length}/10)
          </p>
        </div>
      </div>

      {/* Add new entry */}
      <div className="p-4 border-b border-border/50">
        <AddBlacklistForm onAdd={onAddEntry} disabled={entries.length >= 10} />
      </div>

      {/* List */}
      <ScrollArea className="flex-1">
        <div className="p-4 space-y-2">
          <AnimatePresence mode="popLayout">
            {entries.map((entry, index) => (
              <motion.div
                key={entry.battletag}
                layout
                initial={{ opacity: 0, scale: 0.95 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.95, x: -20 }}
                transition={{ duration: 0.2, delay: index * 0.03 }}
                className="group p-3 rounded-lg bg-secondary/50 border border-border/50 hover:border-destructive/30 transition-colors"
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2 mb-1">
                      <AlertTriangle className="h-3.5 w-3.5 text-destructive shrink-0" />
                      <span className="font-mono text-sm text-foreground truncate">
                        {entry.battletag}
                      </span>
                    </div>
                    <Input
                      placeholder="메모 추가..."
                      defaultValue={entry.memo}
                      onBlur={(e) => {
                        if (e.target.value !== entry.memo) {
                          onUpdateMemo(entry.battletag, e.target.value);
                        }
                      }}
                      className="h-7 text-xs bg-muted/50 border-border/50 focus:border-primary mt-2"
                    />
                  </div>

                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={() => onRemove(entry.battletag)}
                    className="shrink-0 h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity text-muted-foreground hover:text-success hover:bg-success/10"
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              </motion.div>
            ))}
          </AnimatePresence>

          {entries.length === 0 && (
            <div className="py-12 text-center">
              <Shield className="h-12 w-12 text-muted-foreground/30 mx-auto mb-3" />
              <p className="text-sm text-muted-foreground">
                블랙리스트가 비어있습니다
              </p>
            </div>
          )}
        </div>
      </ScrollArea>
    </motion.div>
  );
};

// Sub-component for adding new entries
interface AddBlacklistFormProps {
  onAdd: (battletag: string, memo: string) => void;
  disabled: boolean;
}

const AddBlacklistForm = ({ onAdd, disabled }: AddBlacklistFormProps) => {
  const handleSubmit = (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const form = e.currentTarget;
    const formData = new FormData(form);
    const battletag = formData.get("battletag") as string;
    const memo = formData.get("memo") as string;

    if (battletag.trim()) {
      onAdd(battletag.trim(), memo.trim());
      form.reset();
    }
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-2">
      <div className="flex gap-2">
        <Input
          name="battletag"
          placeholder="배틀태그#1234"
          disabled={disabled}
          className="h-8 text-sm bg-secondary/50 border-border/50 focus:border-primary"
        />
        <Button
          type="submit"
          disabled={disabled}
          size="sm"
          className="h-8 shrink-0"
        >
          <Plus className="h-4 w-4" />
        </Button>
      </div>
      <Input
        name="memo"
        placeholder="메모 (선택사항)"
        disabled={disabled}
        className="h-7 text-xs bg-secondary/50 border-border/50 focus:border-primary"
      />
    </form>
  );
};
