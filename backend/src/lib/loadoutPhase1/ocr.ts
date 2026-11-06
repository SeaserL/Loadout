// SPDX-License-Identifier: Apache-2.0
// The modality bridge's local mode (full edition, ExecPlan 071): instead of asking a vision model
// to describe an image, read the text in it on this computer with PaddleOCR (PP-OCRv4, via
// @gutenye/ocr-node from the AI pack). The image is decoded in memory and never written to disk;
// remote image URLs are not fetched, so such an image is left as it was.
import { aiPackState } from "./aiPack";
import { kvGet } from "./control/shared";

/** "describe" asks the chosen vision model; "ocr" reads the text locally. */
export const modalityMode = () => kvGet<string>("features.modality.mode", "describe");
export const localOcrActive = () => modalityMode() === "ocr" && aiPackState().installed;

type Engine = { detect: (image: { data: Buffer; width: number; height: number }) => Promise<{ texts: { text: string }[] }> };
let engine: Promise<Engine> | null = null;

function load(): Promise<Engine> {
  engine ??= (async () => {
    // Assembled at run time so the build does not try to resolve the optional AI pack.
    const ocrName = "@gutenye/" + "ocr-node";
    const sharpName = "sh" + "arp";
    const [{ default: Ocr }] = await Promise.all([
      import(/* webpackIgnore: true */ ocrName),
      import(/* webpackIgnore: true */ sharpName),
    ]);
    return (await Ocr.create()) as Engine;
  })().catch((error) => {
    engine = null;
    throw error;
  });
  return engine;
}

function decodeDataUri(url: string): Buffer {
  const match = /^data:image\/[a-z0-9.+-]+;base64,(.+)$/i.exec(url);
  if (!match) throw Error("only inline images are read locally");
  return Buffer.from(match[1], "base64");
}

/** The text found in one image, in reading order; what the bridge puts in place of the image. */
export async function ocrImage(imageUrl: string): Promise<string> {
  const bytes = decodeDataUri(imageUrl);
  const [ocr, sharpModule] = await Promise.all([load(), import(/* webpackIgnore: true */ "sh" + "arp")]);
  const sharp = (sharpModule as any).default ?? sharpModule;
  const { data, info } = await sharp(bytes).ensureAlpha(1).raw().toBuffer({ resolveWithObject: true });
  const { texts } = await ocr.detect({ data, width: info.width, height: info.height });
  const lines = texts.map((t) => t.text.trim()).filter(Boolean);
  return lines.length > 0 ? `图片中的文字：\n${lines.join("\n")}` : "（图片中没有识别到文字）";
}
