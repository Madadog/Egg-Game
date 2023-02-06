// Copyright (c) 2023 Adam Godwin <evilspamalt/at/gmail.com>
//
// This file is part of Egg Game - https://github.com/Madadog/Egg-Game/
//
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option) any later
// version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// this program. If not, see <https://www.gnu.org/licenses/>.

// Strings directly printed with `print_raw()` must end with a
// null byte `\0`, while strings printed by the game's dialogue
// system must not.
pub static GAME_TITLE: &str = "EGG GAME\0";
pub static MENU_PLAY: &str = "Play\0";
pub static MENU_OPTIONS: &str = "Options\0";
pub static MENU_BACK: &str = "Back\0";
pub static OPTIONS_FONT_SIZE: &str = "Toggle Font Size\0";
pub static OPTIONS_FONT_FIXED: &str = "Toggle Fixed Font size\0";
pub static OPTIONS_RESET: &str = "Erase Data\0";
pub static OPTIONS_RESET_SURE: &str = "Erase Data (Press again to confirm)\0";
pub static OPTIONS_LOSE_DATA: &str = "You'll lose all data.\0";
pub static BEDROOM_MATTRESS: &str = "You can't get to sleep.";
pub static BEDROOM_TROLLEY: &str = "It's your baby bro's cot.";
pub static BEDROOM_CLOSET: &str = "Everything you have is in here.";
pub static BEDROOM_WINDOW: &str = "It's a beautiful day... \n            \n... Outside.";
pub static HOUSE_STAIRWELL_WINDOW: &str = "The painting, with the window beside, serves as a ceaseless reminder of mankind's fundamental limitations.";
pub static HOUSE_STAIRWELL_WINDOW2: &str = "By a twisted error of design, the view here lines up precisely with the neighbours' bathroom window.";
pub static HOUSE_STAIRWELL_DOOR: &str = "You shouldn't go in there.";
pub static HOUSE_LIVING_ROOM_COUCH: &str = "He's busy.";
pub static HOUSE_LIVING_ROOM_TV: &str = "It's a cartoon of some sort. The protagonist is still charging up his main attack.";
pub static HOUSE_LIVING_ROOM_WINDOW: &str = "You have very strong opinions about gothic windows. None of them are good.";
pub static UNKNOWN_1: &str = "The wispy ethers of your moral fibre hold this door shut.";
pub static SM_COIN_RETURN: &str = "There's no money in the coin return slot.";
pub static SM_FRUIT_BASKET: &str = "They're not fresh.";
pub static SM_MAIN_WINDOW: &str = "It's yellow outside.";
pub static SM_FRIDGE_1: &str =
    "If you blow on the glass, it fogs up, revealing all the fingerprints left by prior employees.";
pub static SM_FRIDGE_2: &str =
    "A note on the front says \"Out of Order\", followed by a smiley face. A sickening contrast.";
pub static SM_VENDING_MACHINE: &str =
    "The blurb reads \"It's SodaTime!\". The machine is filled to the brim with cans of motor oil.";
pub static CONSTRUCTION_1: &str =
    "Looks like the creator didn't put too much effort into this part of the map.";
pub static CONSTRUCTION_2: &str = "Looks like it's still under construction.";
pub static EMERGENCY_EXIT: &str =
    "This is an emergency exit. It's not an emergency right now. Ergo, you cannot use the exit.";
pub static SM_HALL_SHELF: &str =
    "There's a single bottle of floor cleaner. And no mop.";
pub static SM_HALL_WINDOW: &str = "Looks like this window has been recently painted over.";
pub static EGG_1: &str = "It's floating.";
pub static SM_STOREROOM_SHELF: &str = "They're all out of Keratin Krunch.";
pub static SM_TITLE: &str = "S____MAR__T";
pub static INSTRUCTIONS: &str = "Instructions\n\n\nArrow keys: Move around.\n\n[Z]: Interact.\n\n[X]: Skip text.\n\n\nRemember to get regular sleep.\n\n\n\n    Press any button to continue.\0";
// pub static _: &str = "";
