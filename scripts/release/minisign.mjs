// Checks an updater signature the way an installed copy of the app will, without any package:
// Tauri signs update files with minisign (Ed25519 over the BLAKE2b-512 of the file), and both the
// public key and a `.sig` file are the base64 of a small text file.
//
//   public key   untrusted comment: minisign public key: <id>
//                <base64: "Ed", 8 bytes of key id, 32 bytes of key>
//
//   signature    untrusted comment: signature from tauri secret key
//                <base64: "ED" (the file was hashed first) or "Ed", key id, 64 bytes of signature>
//                trusted comment: timestamp:<seconds>\tfile:<name>\tversion:<app version>
//                <base64: 64 bytes, a signature over the first signature and the trusted comment>
//
// The trusted comment is covered by the second signature, so the version in it can be believed
// once that signature holds. An installed copy with `requireSignedVersion` (every build since the
// updater was switched on) refuses an update whose signature carries no version or another one.
import { createHash, createPublicKey, verify } from 'node:crypto';

const text = (file) => Buffer.from(String(file).trim(), 'base64').toString('utf8').split('\n').map((l) => l.replace(/\r$/, '')).filter((l) => l.length);

function publicKey(file) {
  const raw = Buffer.from(text(file)[1] ?? '', 'base64');
  if (raw.length !== 42 || raw.subarray(0, 2).toString('latin1') !== 'Ed') return null;
  return { id: raw.subarray(2, 10), key: raw.subarray(10) };
}

function signature(file) {
  const lines = text(file);
  const raw = Buffer.from(lines[1] ?? '', 'base64');
  const how = raw.subarray(0, 2).toString('latin1');
  const comment = lines[2] ?? '';
  const over = Buffer.from(lines[3] ?? '', 'base64');
  if (raw.length !== 74 || !['Ed', 'ED'].includes(how) || !comment.startsWith('trusted comment: ') || over.length !== 64) return null;
  return { hashed: how === 'ED', id: raw.subarray(2, 10), sig: raw.subarray(10), comment: comment.slice('trusted comment: '.length), over };
}

/** An Ed25519 public key object from its 32 raw bytes. */
const ed25519 = (key) => createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), key]), format: 'der', type: 'spki' });

/**
 * Does `signatureFile` (the content of a `.sig`) sign `data` with the key of `publicKeyFile`
 * (the content of a `.pub`)? Returns `{ ok, why?, version, file }`; `version` and `file` are
 * what the signature itself says it was made for, or null.
 */
export function checkSignature({ publicKeyFile, signatureFile, data }) {
  const pub = publicKey(publicKeyFile);
  if (!pub) return { ok: false, why: 'the public key cannot be read' };
  const sig = signature(signatureFile);
  if (!sig) return { ok: false, why: 'the signature cannot be read' };
  if (!pub.id.equals(sig.id)) return { ok: false, why: 'it was signed with another key' };
  const key = ed25519(pub.key);
  const signed = sig.hashed ? createHash('blake2b512').update(data).digest() : data;
  if (!verify(null, signed, key, sig.sig)) return { ok: false, why: 'the signature does not match the file' };
  if (!verify(null, Buffer.concat([sig.sig, Buffer.from(sig.comment, 'utf8')]), key, sig.over)) return { ok: false, why: 'the signed comment was changed' };
  const field = (name) => {
    const found = sig.comment.split('\t').find((f) => f.startsWith(`${name}:`));
    return found ? found.slice(name.length + 1) : null;
  };
  return { ok: true, version: field('version'), file: field('file') };
}
