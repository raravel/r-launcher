import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";
import { AlertTriangle } from "lucide-react";
import type { RoomUser } from "./UserListTable";

interface AddBlacklistDialogProps {
  user: RoomUser | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: (battletag: string, memo: string) => void;
}

export const AddBlacklistDialog = ({
  user,
  open,
  onOpenChange,
  onConfirm,
}: AddBlacklistDialogProps) => {
  const [memo, setMemo] = useState("");

  const handleConfirm = () => {
    if (user) {
      onConfirm(user.battletag, memo.trim());
      setMemo("");
      onOpenChange(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="bg-card border-border sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 font-display">
            <AlertTriangle className="h-5 w-5 text-destructive" />
            블랙리스트 추가
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-4">
          <div className="p-3 rounded-lg bg-secondary/50 border border-border">
            <p className="text-xs text-muted-foreground mb-1">대상 유저</p>
            <p className="font-mono text-sm text-primary">{user?.battletag}</p>
            <p className="text-sm text-foreground mt-1">{user?.nickname}</p>
          </div>

          <div className="space-y-2">
            <Label htmlFor="memo" className="text-sm text-foreground">
              메모 (선택사항)
            </Label>
            <Textarea
              id="memo"
              value={memo}
              onChange={(e) => setMemo(e.target.value)}
              placeholder="차단 사유를 입력하세요..."
              className="bg-secondary border-border focus:border-primary resize-none"
              rows={3}
            />
          </div>
        </div>

        <DialogFooter className="gap-2 sm:gap-0">
          <Button
            variant="ghost"
            onClick={() => onOpenChange(false)}
            className="text-muted-foreground"
          >
            취소
          </Button>
          <Button
            onClick={handleConfirm}
            className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
          >
            차단하기
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
