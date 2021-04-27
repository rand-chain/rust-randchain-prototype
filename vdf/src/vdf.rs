use rug::Integer;
use std::vec::Vec;

use super::config::MODULUS;
use super::util;

pub type Proof = Vec<Integer>;

pub fn eval(g: &Integer, t: u32) -> Integer {
    let mut y = g.clone();
    for _ in 0..t {
        y = y.clone() * y.clone();
        y = y.div_rem_floor(MODULUS.clone()).1;
    }

    y
}

pub fn prove(g: &Integer, y: &Integer, iterations: u32) -> Proof {
    let (mut x_i, mut y_i) = (g.clone(), y.clone());
    let mut proof = Proof::new();

    let mut t = iterations;
    let two = Integer::from(2);
    while t >= 2 {
        let two_exp = Integer::from(1) << (t / 2); // 2^(t/2)
        let mu_i = x_i.clone().pow_mod(&two_exp, &MODULUS).unwrap();

        let r_i = util::hash_fs(&[&x_i, &y_i, &mu_i]);

        let xi_ri = x_i.clone().pow_mod(&r_i, &MODULUS).unwrap();
        x_i = (xi_ri * mu_i.clone()).div_rem_floor(MODULUS.clone()).1;

        let mui_ri = mu_i.clone().pow_mod(&r_i, &MODULUS).unwrap();
        y_i = (mui_ri * y_i.clone()).div_rem_floor(MODULUS.clone()).1;

        t = t / 2;
        if (t % 2 != 0) && (t != 1) {
            t += 1;
            y_i = y_i.clone().pow_mod(&two, &MODULUS).unwrap();
        }

        proof.push(mu_i);
    }

    proof
}

pub fn verify(g: &Integer, y: &Integer, iterations: u32, proof: &Proof) -> bool {
    let (mut x_i, mut y_i) = (g.clone(), y.clone());
    let mut t = iterations;
    let two = Integer::from(2);
    for mu_i in proof {
        let r_i = util::hash_fs(&[&x_i, &y_i, &mu_i]);

        let xi_ri = x_i.clone().pow_mod(&r_i, &MODULUS).unwrap();
        x_i = (xi_ri * mu_i.clone()).div_rem_floor(MODULUS.clone()).1;

        let mui_ri = mu_i.clone().pow_mod(&r_i, &MODULUS).unwrap();
        y_i = (mui_ri * y_i.clone()).div_rem_floor(MODULUS.clone()).1;

        t = t / 2;
        if (t % 2 != 0) && (t != 1) {
            t += 1;
            y_i = y_i.clone().pow_mod(&two, &MODULUS).unwrap();
        }
    }

    y_i == x_i.pow_mod(&two, &MODULUS).unwrap()
}
