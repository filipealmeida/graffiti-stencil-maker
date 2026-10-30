const ADJECTIVES = ['admiring', 'bold', 'brave', 'calm', 'clever', 'cosmic', 'crisp', 'daring', 'eager', 'elegant', 'fervent', 'focused', 'gallant', 'gentle', 'happy', 'hopeful', 'jolly', 'keen', 'lively', 'lucid', 'mellow', 'nimble', 'noble', 'patient', 'quirky', 'radiant', 'serene', 'sharp', 'sleepy', 'steady', 'sunny', 'swift', 'upbeat', 'vivid', 'witty', 'zesty']
const NOUNS = ['ada', 'archimedes', 'babbage', 'bohr', 'curie', 'darwin', 'dijkstra', 'einstein', 'euclid', 'euler', 'faraday', 'fermat', 'franklin', 'galileo', 'gauss', 'hopper', 'hypatia', 'kepler', 'lovelace', 'maxwell', 'newton', 'noether', 'pascal', 'planck', 'ramanujan', 'tesla', 'turing', 'volta', 'wiles', 'zuse']

const pick = <T,>(a: readonly T[]) => a[Math.floor(Math.random() * a.length)]

/** A name like docker's: adjective_surname. */
export const randomName = () => `${pick(ADJECTIVES)}_${pick(NOUNS)}`
