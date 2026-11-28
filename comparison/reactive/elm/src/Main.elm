port module Main exposing (main)

{-| The reactive scenarios the Elm way: no graph, every message runs
`update`, and the derived values are recomputed from the whole model, as a
`view` would. A "watch" adds the derived value when it differs from the last
one seen. The scenario name comes in as a flag; the checksum goes out.
-}

import Platform


port done : Int -> Cmd msg


n : Int
n =
    1000


type alias Watch =
    { last : Maybe Int, seen : Int }


observe : Int -> Watch -> Watch
observe v w =
    if w.last == Just v then
        w

    else
        { last = Just v, seen = w.seen + v }


{-| The derived chain d0 = s + 1, d(i) = d(i-1) + 1, recomputed in full. -}
chainValue : Int -> Int
chainValue s =
    List.foldl (\_ d -> d + 1) (s + 1) (List.range 1 (n - 1))


fanValue : Int -> Int
fanValue s =
    List.range 0 (n - 1) |> List.map (\i -> s + i) |> List.sum


updates : (Int -> Int) -> Int
updates derive =
    List.foldl (\v w -> observe (derive v) w) (observe (derive 0) (Watch Nothing 0)) (List.range 1 1000)
        |> .seen


counters : () -> Int
counters _ =
    let
        total xs =
            List.sum (List.map (\c -> c * 2) xs)

        made =
            List.range 0 (n - 1)

        start =
            observe (total made) (observe (total []) (Watch Nothing 0))

        tick _ ( xs, w ) =
            let
                ys =
                    List.map (\c -> c + 1) xs
            in
            ( ys, observe (total ys) w )
    in
    List.foldl tick ( made, start ) (List.range 1 100) |> Tuple.second |> .seen


type alias Todo =
    { id : Int, done : Bool }


todos : () -> Int
todos _ =
    let
        remaining xs =
            List.length (List.filter (not << .done) xs)

        step f ( xs, w ) =
            let
                ys =
                    f xs
            in
            ( ys, observe (remaining ys) w )

        adds =
            List.range 0 (n - 1) |> List.map (\i -> \xs -> xs ++ [ Todo i False ])

        toggles =
            List.range 0 (n // 2 - 1)
                |> List.map (\k -> \xs -> List.map (\t -> if t.id == 2 * k then { t | done = not t.done } else t) xs)

        removes =
            List.range 0 (n // 2 - 1) |> List.map (\i -> \xs -> List.filter (\t -> t.id /= i) xs)
    in
    List.foldl step ( [], observe 0 (Watch Nothing 0) ) (adds ++ toggles ++ removes) |> Tuple.second |> .seen


run : String -> Int
run name =
    case name of
        "chain" ->
            updates chainValue

        "fan" ->
            updates fanValue

        "counters" ->
            counters ()

        "todos" ->
            todos ()

        _ ->
            -1


main : Program String () ()
main =
    Platform.worker
        { init = \name -> ( (), done (run name) )
        , update = \_ m -> ( m, Cmd.none )
        , subscriptions = \_ -> Sub.none
        }
