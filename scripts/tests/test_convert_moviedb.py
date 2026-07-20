import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[1]))

from convert_moviedb import convert_movie, iter_json_array, movie_bucket, timestamp_ms


class ConvertMovieDbTests(unittest.TestCase):
    def test_streams_and_maps_fixture(self):
        fixture = Path(__file__).parents[1] / "fixtures" / "moviedb.sample.json"
        movies = list(iter_json_array(fixture))
        self.assertEqual(len(movies), 2)

        document = convert_movie(movies[0])
        self.assertEqual(document["bucket"], "default")
        self.assertEqual(document["oid"], "movie:11")
        self.assertIn("Science Fiction", document["text"])
        self.assertEqual(document["metadata"], {"id": 11})

    def test_converts_release_dates(self):
        self.assertEqual(timestamp_ms("1970-01-01"), 0)
        self.assertEqual(timestamp_ms("1970-01-02"), 86_400_000)
        self.assertEqual(timestamp_ms("invalid"), 0)

    def test_assigns_deterministic_buckets(self):
        self.assertEqual(movie_bucket(11, "movies", 64), "movies:0011")
        self.assertEqual(movie_bucket(75, "movies", 64), "movies:0011")
        self.assertEqual(movie_bucket("movie-id", "movies", 64), "movies:0045")
        self.assertEqual(movie_bucket(11, "default", 1), "default")

        document = convert_movie({"id": 75, "title": "Shard"}, "movies", 64)
        self.assertEqual(document["bucket"], "movies:0011")

    def test_rejects_invalid_bucket_count(self):
        with self.assertRaisesRegex(ValueError, "bucket count"):
            movie_bucket(11, "movies", 0)


if __name__ == "__main__":
    unittest.main()
