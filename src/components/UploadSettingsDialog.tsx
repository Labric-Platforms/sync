import { useState, useEffect, useCallback } from "react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
import { toast } from "sonner";
import { enable as enableAutostart, disable as disableAutostart, isEnabled as isAutostartEnabled } from "@tauri-apps/plugin-autostart";
import { useUploadManager } from "@/hooks/useUploadManager";

interface UploadSettingsSheetProps {
  children: React.ReactNode;
}

function UploadSettingsSheet({ children }: UploadSettingsSheetProps) {
  const { config, isLoading, error, updateConfig } = useUploadManager();

  const [open, setOpen] = useState(false);

  // Draft state
  const [enabled, setEnabled] = useState(false);
  const [ignoreExisting, setIgnoreExisting] = useState(false);
  const [delayInput, setDelayInput] = useState("");
  const [concurrencyInput, setConcurrencyInput] = useState("");
  const [ignoredPatterns, setIgnoredPatterns] = useState<string[]>([]);
  const [newPattern, setNewPattern] = useState("");
  const [launchAtLogin, setLaunchAtLogin] = useState(false);

  const resetDraft = useCallback(() => {
    if (config) {
      setEnabled(config.enabled);
      setIgnoreExisting(config.ignore_existing_files);
      setDelayInput((config.upload_delay_ms / 1000).toString());
      setConcurrencyInput(config.max_concurrent_uploads.toString());
      setIgnoredPatterns([...config.ignored_patterns]);
      setNewPattern("");
    }
  }, [config]);

  useEffect(() => {
    if (open) {
      resetDraft();
      setDelayError("");
      setConcurrencyError("");
      isAutostartEnabled()
        .then(setLaunchAtLogin)
        .catch((err) => console.error("Failed to read autostart state:", err));
    }
  }, [open, resetDraft]);

  const handleAddPattern = () => {
    const trimmed = newPattern.trim();
    if (trimmed && !ignoredPatterns.includes(trimmed)) {
      setIgnoredPatterns((prev) => [...prev, trimmed]);
      setNewPattern("");
    }
  };

  const [delayError, setDelayError] = useState("");
  const [concurrencyError, setConcurrencyError] = useState("");

  const validate = () => {
    let valid = true;
    const delay = parseFloat(delayInput);
    const concurrency = parseInt(concurrencyInput);

    if (isNaN(delay) || delay < 0) {
      setDelayError("Must be a number ≥ 0.");
      valid = false;
    } else {
      setDelayError("");
    }

    if (isNaN(concurrency) || concurrency < 1 || concurrency > 20) {
      setConcurrencyError("Must be between 1 and 20.");
      valid = false;
    } else {
      setConcurrencyError("");
    }

    return valid;
  };

  const handleSave = async () => {
    if (!config || !validate()) return;
    const delay = parseFloat(delayInput);
    const concurrency = parseInt(concurrencyInput);

    try {
      await updateConfig({
        ...config,
        enabled,
        ignore_existing_files: ignoreExisting,
        upload_delay_ms: Math.round(delay * 1000),
        max_concurrent_uploads: concurrency,
        ignored_patterns: ignoredPatterns,
      });
      try {
        if (launchAtLogin) {
          await enableAutostart();
        } else {
          await disableAutostart();
        }
      } catch (err) {
        console.error("Failed to toggle autostart:", err);
        toast.error("Failed to update launch-at-login setting");
      }
      toast.success("Settings saved");
      setOpen(false);
    } catch (err) {
      console.error("Failed to save settings:", err);
      toast.error("Failed to save settings");
    }
  };

  if (isLoading || error || !config) {
    return (
      <Sheet open={open} onOpenChange={setOpen}>
        <SheetTrigger asChild>{children}</SheetTrigger>
        <SheetContent>
          <SheetHeader>
            <SheetTitle className="font-title">Upload Settings</SheetTitle>
            <SheetDescription className="sr-only">
              Configure upload behavior and file filters
            </SheetDescription>
          </SheetHeader>
        </SheetContent>
      </Sheet>
    );
  }

  return (
    <Sheet open={open} onOpenChange={setOpen}>
      <Tooltip>
        <TooltipTrigger asChild>
          <div tabIndex={-1}>
            <SheetTrigger asChild>{children}</SheetTrigger>
          </div>
        </TooltipTrigger>
        <TooltipContent collisionPadding={8}>Upload Settings</TooltipContent>
      </Tooltip>
      <SheetContent className="w-[400px] sm:w-[540px] flex flex-col">
        <SheetHeader>
          <SheetTitle className="font-title">Upload Settings</SheetTitle>
          <SheetDescription className="sr-only">
            Configure upload behavior and file filters
          </SheetDescription>
        </SheetHeader>

        <div className="flex-1 overflow-y-auto px-4 pb-4">
          <FieldGroup>
            <Field orientation="horizontal" className="!items-center justify-between gap-4">
              <FieldContent>
                <FieldLabel htmlFor="enable-uploads">Enable Uploads</FieldLabel>
                <FieldDescription>Upload files on change.</FieldDescription>
              </FieldContent>
              <Switch
                id="enable-uploads"
                checked={enabled}
                onCheckedChange={setEnabled}
              />
            </Field>

            <Field orientation="horizontal" className="!items-center justify-between gap-4">
              <FieldContent>
                <FieldLabel htmlFor="ignore-existing">Ignore Existing Files</FieldLabel>
                <FieldDescription>Skip files already in the folder.</FieldDescription>
              </FieldContent>
              <Switch
                id="ignore-existing"
                checked={ignoreExisting}
                onCheckedChange={setIgnoreExisting}
              />
            </Field>

            <Field orientation="horizontal" className="!items-center justify-between gap-4">
              <FieldContent>
                <FieldLabel htmlFor="launch-at-login">Launch at Login</FieldLabel>
                <FieldDescription>Start Labric Sync when you log in.</FieldDescription>
              </FieldContent>
              <Switch
                id="launch-at-login"
                checked={launchAtLogin}
                onCheckedChange={setLaunchAtLogin}
              />
            </Field>

            <Field data-invalid={!!delayError || undefined}>
              <FieldLabel htmlFor="delayInput">Upload Delay (seconds)</FieldLabel>
              <Input
                id="delayInput"
                type="number"
                value={delayInput}
                onChange={(e) => { setDelayInput(e.target.value); setDelayError(""); }}
                placeholder="2"
                min="0"
                step="0.1"
                aria-invalid={!!delayError}
              />
              {delayError && <FieldError>{delayError}</FieldError>}
            </Field>

            <Field data-invalid={!!concurrencyError || undefined}>
              <FieldLabel htmlFor="concurrencyInput">Max Concurrent Uploads</FieldLabel>
              <Input
                id="concurrencyInput"
                type="number"
                value={concurrencyInput}
                onChange={(e) => { setConcurrencyInput(e.target.value); setConcurrencyError(""); }}
                placeholder="5"
                min="1"
                max="20"
                aria-invalid={!!concurrencyError}
              />
              {concurrencyError && <FieldError>{concurrencyError}</FieldError>}
            </Field>

            <Field>
              <FieldLabel htmlFor="newPattern">Ignored Patterns</FieldLabel>
              <div className="flex gap-2">
                <Input
                  id="newPattern"
                  type="text"
                  value={newPattern}
                  onChange={(e) => setNewPattern(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      handleAddPattern();
                    }
                  }}
                  placeholder="*.tmp, .git/**"
                  className="flex-1"
                />
                <Button onClick={handleAddPattern} variant="secondary">
                  Add
                </Button>
              </div>
              {ignoredPatterns.length > 0 && (
                <div className="flex flex-wrap gap-1.5">
                  {ignoredPatterns.map((pattern, index) => (
                    <Badge
                      key={index}
                      variant="secondary"
                      className="cursor-pointer"
                      onClick={() =>
                        setIgnoredPatterns((prev) =>
                          prev.filter((p) => p !== pattern)
                        )
                      }
                    >
                      {pattern} ×
                    </Badge>
                  ))}
                </div>
              )}
            </Field>
          </FieldGroup>
        </div>

        <SheetFooter className="border-t flex-row gap-2 px-4 py-4">
          <Button variant="outline" className="flex-1 rounded-full" onClick={() => setOpen(false)}>
            Cancel
          </Button>
          <Button className="flex-1 rounded-full" onClick={handleSave}>
            Save
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}

export default UploadSettingsSheet;
