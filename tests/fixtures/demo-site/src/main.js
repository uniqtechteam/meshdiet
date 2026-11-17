import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { DRACOLoader } from 'three/addons/loaders/DRACOLoader.js';

const renderer = new THREE.WebGLRenderer({ canvas: document.getElementById('view') });
const scene = new THREE.Scene();
const camera = new THREE.PerspectiveCamera(50, innerWidth / innerHeight, 0.1, 100);
camera.position.set(0, 1.5, 4);

const draco = new DRACOLoader();
draco.setDecoderPath('/draco/');
const loader = new GLTFLoader();
loader.setDRACOLoader(draco);

for (const name of ['hero.glb', 'chair.glb', 'sofa.glb', 'lamp.gltf']) {
  loader.load(`/models/${name}`, (gltf) => scene.add(gltf.scene));
}

renderer.setAnimationLoop(() => renderer.render(scene, camera));
