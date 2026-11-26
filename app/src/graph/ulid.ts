const ALPHABET = "0123456789abcdefghjkmnpqrstvwxyz";

export function generateUlid(): string {
  let time = Date.now();
  let timePart = "";
  for (let index = 0; index < 10; index++) {
    timePart = ALPHABET[time % 32] + timePart;
    time = Math.floor(time / 32);
  }
  const random = crypto.getRandomValues(new Uint8Array(16));
  let randomPart = "";
  for (let index = 0; index < 16; index++) {
    randomPart += ALPHABET[random[index] % 32];
  }
  return timePart + randomPart;
}
